//! Signing in: with a key (from the vault or an agent), a stored password and
//! keyboard-interactive questions, asking the user where nothing stored may answer.

use std::sync::Arc;

use russh::client::{self, KeyboardInteractiveAuthResponse};
use russh::keys::{HashAlg, PrivateKeyWithHashAlg};
use russh::{MethodKind, MethodSet};
use zeroize::Zeroizing;

use crate::{AGENT_SIGN_TIMEOUT, Credential, SshError, agent_identities, protocol};

/// How to sign in on one host: a key, a stored password, both or neither (then the user is asked
/// where the server allows it).
pub struct Auth {
    pub key: Option<Credential>,
    pub password: Option<Zeroizing<String>>,
}

impl Auth {
    pub fn key(credential: Credential) -> Self {
        Self {
            key: Some(credential),
            password: None,
        }
    }
}

/// One line of a question from the server.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct AuthPromptLine {
    pub text: String,
    /// Whether what the user types may be shown.
    pub echo: bool,
}

/// Questions the server asks during sign-in (keyboard-interactive), or the password question
/// when the server only takes passwords and none is stored.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct AuthRequest {
    pub host_name: String,
    pub username: String,
    pub name: String,
    pub instructions: String,
    pub prompts: Vec<AuthPromptLine>,
}

/// Asks the user. `None` means the user cancelled.
#[async_trait::async_trait]
pub trait AuthPrompter: Send + Sync {
    async fn answer(&self, request: AuthRequest) -> Option<Vec<Zeroizing<String>>>;
}

/// Answers nothing: sign-in fails where the user would have to be asked.
pub struct NoPrompter;

#[async_trait::async_trait]
impl AuthPrompter for NoPrompter {
    async fn answer(&self, _request: AuthRequest) -> Option<Vec<Zeroizing<String>>> {
        None
    }
}

/// At most this many keyboard-interactive rounds go to the user.
const MAX_ASKED_ROUNDS: usize = 3;

struct Flow<'a> {
    username: &'a str,
    host_name: &'a str,
    prompter: &'a dyn AuthPrompter,
    methods: MethodSet,
    password_rejected: bool,
}

impl Flow<'_> {
    fn offers(&self, method: MethodKind) -> bool {
        self.methods.contains(&method)
    }

    /// Takes the server's answer to a failed attempt: which methods are left.
    fn failed(&mut self, remaining: MethodSet) {
        self.methods = remaining;
    }

    async fn ask(
        &self,
        name: String,
        instructions: String,
        prompts: Vec<AuthPromptLine>,
    ) -> Result<Vec<Zeroizing<String>>, SshError> {
        let count = prompts.len();
        let answers = self
            .prompter
            .answer(AuthRequest {
                host_name: self.host_name.to_owned(),
                username: self.username.to_owned(),
                name,
                instructions,
                prompts,
            })
            .await
            .ok_or(SshError::AuthCancelled)?;
        if answers.len() != count {
            return Err(SshError::AuthCancelled);
        }
        Ok(answers)
    }
}

/// Signs in. The order: the key (`publickey`), then a stored password (`password`), then
/// `keyboard-interactive`, where a stored password answers at most one round and only one with
/// a single hidden question. Without a stored password and with only `password` offered, the
/// user is asked once.
pub(crate) async fn authenticate<H: client::Handler>(
    handle: &mut client::Handle<H>,
    username: &str,
    host_name: &str,
    auth: Auth,
    prompter: &dyn AuthPrompter,
) -> Result<(), SshError> {
    let mut flow = Flow {
        username,
        host_name,
        prompter,
        methods: MethodSet::empty(),
        password_rejected: false,
    };
    let first = match auth.key {
        Some(credential) => publickey(handle, username, credential).await?,
        None => handle.authenticate_none(username).await.map_err(protocol)?,
    };
    match first {
        russh::client::AuthResult::Success => return Ok(()),
        russh::client::AuthResult::Failure {
            remaining_methods, ..
        } => flow.failed(remaining_methods),
    }

    let mut stored = auth.password;
    if let Some(password) = &stored
        && flow.offers(MethodKind::Password)
    {
        match handle
            .authenticate_password(username, password.as_str())
            .await
            .map_err(protocol)?
        {
            russh::client::AuthResult::Success => return Ok(()),
            russh::client::AuthResult::Failure {
                remaining_methods,
                partial_success,
            } => {
                flow.password_rejected = !partial_success;
                flow.failed(remaining_methods);
            }
        }
    }
    if flow.password_rejected {
        stored = None;
    }

    if flow.offers(MethodKind::KeyboardInteractive) {
        let mut asked = 0;
        let mut response = handle
            .authenticate_keyboard_interactive_start(username, None)
            .await
            .map_err(protocol)?;
        loop {
            match response {
                KeyboardInteractiveAuthResponse::Success => return Ok(()),
                KeyboardInteractiveAuthResponse::Failure {
                    remaining_methods, ..
                } => {
                    flow.failed(remaining_methods);
                    break;
                }
                KeyboardInteractiveAuthResponse::InfoRequest {
                    name,
                    instructions,
                    prompts,
                } => {
                    let single_hidden = prompts.len() == 1 && !prompts[0].echo;
                    let answers: Vec<Zeroizing<String>> = if prompts.is_empty() {
                        Vec::new()
                    } else if single_hidden && let Some(password) = stored.take() {
                        vec![password]
                    } else {
                        asked += 1;
                        if asked > MAX_ASKED_ROUNDS {
                            return Err(SshError::AuthFailed);
                        }
                        let lines = prompts
                            .into_iter()
                            .map(|p| AuthPromptLine {
                                text: p.prompt,
                                echo: p.echo,
                            })
                            .collect();
                        flow.ask(name, instructions, lines).await?
                    };
                    // russh takes plain strings; they live only until they are sent.
                    let plain = answers.iter().map(|a| a.to_string()).collect();
                    response = handle
                        .authenticate_keyboard_interactive_respond(plain)
                        .await
                        .map_err(protocol)?;
                }
            }
        }
    }

    if !flow.password_rejected && stored.is_none() && flow.offers(MethodKind::Password) {
        let typed = flow
            .ask(
                String::new(),
                String::new(),
                vec![AuthPromptLine {
                    text: "Password: ".into(),
                    echo: false,
                }],
            )
            .await?;
        let password = typed.into_iter().next().unwrap_or_default();
        match handle
            .authenticate_password(username, password.as_str())
            .await
            .map_err(protocol)?
        {
            russh::client::AuthResult::Success => return Ok(()),
            russh::client::AuthResult::Failure { .. } => return Err(SshError::PasswordRejected),
        }
    }

    Err(if flow.password_rejected {
        SshError::PasswordRejected
    } else {
        SshError::AuthFailed
    })
}

async fn publickey<H: client::Handler>(
    handle: &mut client::Handle<H>,
    username: &str,
    credential: Credential,
) -> Result<russh::client::AuthResult, SshError> {
    let hash = handle
        .best_supported_rsa_hash()
        .await
        .map_err(protocol)?
        .flatten();
    match credential {
        Credential::Key(key) => handle
            .authenticate_publickey(username, PrivateKeyWithHashAlg::new(Arc::new(key), hash))
            .await
            .map_err(protocol),
        Credential::Agent {
            fingerprint,
            mut agent,
        } => {
            let identities = agent_identities(&mut agent).await?;
            let identity = identities
                .into_iter()
                .find(|i| i.public_key().fingerprint(HashAlg::Sha256).to_string() == fingerprint)
                .ok_or(SshError::AgentKeyMissing)?;
            let signed = handle.authenticate_publickey_with(
                username,
                identity.public_key().into_owned(),
                hash,
                &mut agent,
            );
            tokio::time::timeout(AGENT_SIGN_TIMEOUT, signed)
                .await
                .map_err(|_| SshError::AgentUnavailable("no answer while signing".into()))?
                .map_err(|e| match e {
                    russh::AgentAuthError::Key(russh::keys::Error::AgentFailure) => {
                        SshError::AgentRefused
                    }
                    russh::AgentAuthError::Key(other) => {
                        SshError::AgentUnavailable(other.to_string())
                    }
                    other => protocol(other),
                })
        }
    }
}
