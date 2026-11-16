//! A container on a host as the target of a session: SSHCraft signs in to the host and runs
//! `docker exec`, `podman exec` or `kubectl exec` with a terminal. The command is fixed; only the
//! container's name, the namespace and one of a few shells go into it, and only after [`check`]
//! let them through. Names that come from the server (the pick list) are checked the same way.

use serde::{Deserialize, Serialize};

/// The longest container name.
const MAX_NAME: usize = 128;
/// The longest Kubernetes name.
const MAX_K8S_NAME: usize = 253;
/// Names read from one listing.
pub const MAX_LISTED: usize = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Runtime {
    Docker,
    Podman,
    Kubectl,
}

/// The shells a container may be entered with.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Shell {
    #[default]
    Sh,
    Bash,
    Zsh,
    Ash,
}

impl Shell {
    fn program(self) -> &'static str {
        match self {
            Shell::Sh => "sh",
            Shell::Bash => "bash",
            Shell::Zsh => "zsh",
            Shell::Ash => "ash",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Container {
    pub runtime: Runtime,
    /// The container's name (a pod's for Kubernetes).
    pub name: String,
    /// The Kubernetes namespace; the current one when missing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    #[serde(default)]
    pub shell: Shell,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ContainerError {
    #[error("a container name has letters, digits, dots, dashes and underscores and starts with a letter or digit")]
    Name,
    #[error("a Kubernetes name has lowercase letters, digits, dots and dashes and starts and ends with a letter or digit")]
    KubernetesName,
    #[error("only Kubernetes has namespaces")]
    Namespace,
}

/// A Docker or Podman name: `[A-Za-z0-9][A-Za-z0-9_.-]*`, at most 128.
fn container_name_ok(name: &str) -> bool {
    name.len() <= MAX_NAME
        && name.bytes().next().is_some_and(|b| b.is_ascii_alphanumeric())
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-'))
}

/// A Kubernetes object name: lowercase letters, digits, `.` and `-`, starting and ending with a
/// letter or digit, at most 253.
fn k8s_name_ok(name: &str) -> bool {
    name.len() <= MAX_K8S_NAME
        && name.bytes().next().is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        && name.bytes().last().is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'-'))
}

/// Whether `container` is safe inside the fixed command.
pub fn check(container: &Container) -> Result<(), ContainerError> {
    match container.runtime {
        Runtime::Docker | Runtime::Podman => {
            if container.namespace.is_some() {
                return Err(ContainerError::Namespace);
            }
            container_name_ok(&container.name).then_some(()).ok_or(ContainerError::Name)
        }
        Runtime::Kubectl => {
            let names = std::iter::once(container.name.as_str()).chain(container.namespace.as_deref());
            names.into_iter().all(k8s_name_ok).then_some(()).ok_or(ContainerError::KubernetesName)
        }
    }
}

impl Container {
    /// The command that opens a shell in the container; `None` when [`check`] refuses.
    pub fn command(&self) -> Option<String> {
        check(self).ok()?;
        let shell = self.shell.program();
        Some(match self.runtime {
            Runtime::Docker => format!("docker exec -it {} {shell}", self.name),
            Runtime::Podman => format!("podman exec -it {} {shell}", self.name),
            Runtime::Kubectl => match &self.namespace {
                Some(ns) => format!("kubectl exec -it -n {ns} {} -- {shell}", self.name),
                None => format!("kubectl exec -it {} -- {shell}", self.name),
            },
        })
    }
}

/// The command that lists the containers (pods) a runtime runs, one name per line; for
/// Kubernetes in `namespace` when given. `None` when the namespace fails the check, or is given
/// for Docker or Podman.
pub fn list_command(runtime: Runtime, namespace: Option<&str>) -> Option<String> {
    match (runtime, namespace) {
        (Runtime::Docker, None) => Some("docker ps --format '{{.Names}}'".into()),
        (Runtime::Podman, None) => Some("podman ps --format '{{.Names}}'".into()),
        (Runtime::Kubectl, None) => Some("kubectl get pods -o name".into()),
        (Runtime::Kubectl, Some(ns)) => k8s_name_ok(ns).then(|| format!("kubectl get pods -n {ns} -o name")),
        (Runtime::Docker | Runtime::Podman, Some(_)) => None,
    }
}

/// The names in a listing, each checked; what does not pass is left out, at most [`MAX_LISTED`].
/// When the output was `cut` at its limit, the last line may be part of a name and is left out.
pub fn parse_list(runtime: Runtime, output: &str, cut: bool) -> Vec<String> {
    let whole = if cut { output.rfind('\n').map_or("", |end| &output[..end]) } else { output };
    whole
        .lines()
        .map(str::trim)
        .map(|l| if runtime == Runtime::Kubectl { l.strip_prefix("pod/").unwrap_or(l) } else { l })
        .filter(|name| match runtime {
            Runtime::Kubectl => k8s_name_ok(name),
            Runtime::Docker | Runtime::Podman => container_name_ok(name),
        })
        .take(MAX_LISTED)
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn docker(name: &str) -> Container {
        Container { runtime: Runtime::Docker, name: name.into(), namespace: None, shell: Shell::Bash }
    }

    #[test]
    fn names_are_checked() {
        for good in ["web", "web_1.prod-a", "0db"] {
            assert_eq!(check(&docker(good)), Ok(()), "{good}");
        }
        for bad in ["", "-it", "_web", "a b", "a;id", "$(id)", "x`id`", "ä", &"x".repeat(129)] {
            assert_eq!(check(&docker(bad)), Err(ContainerError::Name), "{bad}");
        }
        let pod = |name: &str, ns: Option<&str>| Container {
            runtime: Runtime::Kubectl,
            name: name.into(),
            namespace: ns.map(str::to_string),
            shell: Shell::Sh,
        };
        assert_eq!(check(&pod("api-7d9f", Some("prod"))), Ok(()));
        assert_eq!(check(&pod("Api", None)), Err(ContainerError::KubernetesName));
        assert_eq!(check(&pod("api", Some("-n"))), Err(ContainerError::KubernetesName));
        assert_eq!(check(&pod("api-", None)), Err(ContainerError::KubernetesName));
        let mut with_ns = docker("web");
        with_ns.namespace = Some("prod".into());
        assert_eq!(check(&with_ns), Err(ContainerError::Namespace));
    }

    #[test]
    fn the_command_is_fixed_around_the_names() {
        assert_eq!(docker("web").command().as_deref(), Some("docker exec -it web bash"));
        let podman = Container { runtime: Runtime::Podman, name: "db".into(), namespace: None, shell: Shell::Sh };
        assert_eq!(podman.command().as_deref(), Some("podman exec -it db sh"));
        let pod = Container { runtime: Runtime::Kubectl, name: "api".into(), namespace: Some("prod".into()), shell: Shell::Ash };
        assert_eq!(pod.command().as_deref(), Some("kubectl exec -it -n prod api -- ash"));
        assert_eq!(docker("a;id").command(), None);
    }

    #[test]
    fn listings_keep_only_names_that_pass() {
        assert_eq!(parse_list(Runtime::Docker, "web\ndb_1\n\nbad name\n$(id)\n", false), vec!["web".to_string(), "db_1".to_string()]);
        assert_eq!(parse_list(Runtime::Kubectl, "pod/api-1\npod/Bad\nworker\n", false), vec!["api-1".to_string(), "worker".to_string()]);
        let many = "c\n".repeat(MAX_LISTED + 10);
        assert_eq!(parse_list(Runtime::Docker, &many, false).len(), MAX_LISTED);
    }
}

#[cfg(test)]
mod listing_tests {
    use super::*;

    #[test]
    fn kubernetes_lists_the_namespace_typed() {
        assert_eq!(list_command(Runtime::Kubectl, Some("prod")).as_deref(), Some("kubectl get pods -n prod -o name"));
        assert_eq!(list_command(Runtime::Kubectl, None).as_deref(), Some("kubectl get pods -o name"));
        assert_eq!(list_command(Runtime::Kubectl, Some("x;id")), None, "a namespace that fails the check lists nothing");
        assert_eq!(list_command(Runtime::Docker, None).as_deref(), Some("docker ps --format '{{.Names}}'"));
    }

    #[test]
    fn a_cut_last_line_is_left_out() {
        assert_eq!(parse_list(Runtime::Docker, "web\ndb\nwe", true), vec!["web".to_string(), "db".to_string()]);
        assert_eq!(parse_list(Runtime::Docker, "web\ndb", false), vec!["web".to_string(), "db".to_string()]);
    }
}
