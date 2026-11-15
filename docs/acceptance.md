# Acceptance list

The rules that came along from the Tauri version (plans 1 to 15) and the test that holds each
one. A rule without a test here is a gap; `docs/gaps.md` names it.

## Connections and keys

| Rule | Test |
|---|---|
| A changed server fingerprint always aborts | `mismatch_ends_without_prompt` (`crates/engine/src/sessions_tests.rs`), `a_changed_host_key_is_never_accepted` (`crates/ui-egui/tests/session.rs`) |
| … also on reconnect | `reconnect_into_changed_host_key_stops` (`crates/engine/src/sessions_tests.rs`) |
| A missing master key while keys exist: no new one is made | `unlock_refuses_new_master_key_when_secrets_exist` (`crates/store/src/vault_tests.rs`), `open_without_master_key_creates_none` (`crates/engine/src/tests.rs`) |

## Sync

| Rule | Test |
|---|---|
| End-to-end encryption: the server never sees a password | `server_never_sees_password_plaintext` (`crates/sync/tests/credentials.rs`) |
| … nor a host's name, address, user or tags (also not base64-encoded) | `server_never_sees_host_content`, `the_leak_scan_finds_base64_too` (`crates/sync/tests/credentials.rs`) |
| A changed record is checked | `ciphertext_tampering_is_rejected` (`crates/proto/tests/record.rs`) |
| Rollback protection | `pull_rejects_a_rollback` (`crates/sync/tests/pull.rs`), `foreign_log_rollback_is_refused` (`crates/sync/tests/areas.rs`), `shorter_log_than_known_is_a_rollback` (`crates/proto/tests/devicelog.rs`), `tracker_rejects_rollback`, `manifest_tracking_rejects_rollback_and_fork` (`crates/proto/tests/record.rs`) |
| Revoking a device rotates the keys | `revoked_device_loses_access`, `rotation_keeps_data_readable`, `retry_after_a_failed_rotation_still_rotates` (`crates/sync/tests/devices.rs`), `revoke_rotates_every_owned_area` (`crates/sync/tests/areas.rs`) |
| Revoking a share rotates its key | `revoked_recipient_cannot_read_new_states` (`crates/sync/tests/share_rights.rs`), `members_revoked_device_cannot_read_new_data` (`crates/sync/tests/share_devices.rs`) |

## Session logs

| Rule | Test |
|---|---|
| Only output is recorded, never what is typed | `typed_input_never_reaches_the_log` (`crates/engine/src/sessions_tests.rs`) |
| Retention deletes at the server too | `expired_synced_logs_are_deleted_at_their_server_too`, `housekeeping_deletes_only_expired_logs` (`crates/engine/src/recording_tests.rs`) |
| Logs sync only in the personal area | `a_log_of_a_shared_host_stays_personal` (`crates/sync/tests/logs.rs`) |
| Searching holds no database lock | `search_does_not_hold_the_store_lock` (`crates/engine/src/cmd/log.rs`) |

## Agents (the review of plan 15)

| Rule | Test |
|---|---|
| `exec` returns standard error and the exit code | `exec_waits_as_long_as_asked_and_reports_errors_and_status` (`crates/engine/src/sessions_tests.rs`) |
| `save_host` overwrites nothing it was not given | `a_change_touches_only_what_it_names_and_shows_it` (`crates/engine/src/gate/gate_tests.rs`) |
| … and never points stored sign-in data elsewhere | `stored_sign_in_data_is_never_pointed_elsewhere` (`crates/engine/src/gate/gate_tests.rs`) |
| Jump hosts do not reveal hidden hosts | `a_host_behind_a_hidden_jump_host_is_hidden_too`, `hidden_hosts_are_unknown_everywhere` (`crates/engine/src/gate/gate_tests.rs`) |
| `delete_group` deletes nothing hidden | `a_group_with_something_hidden_below_is_not_deleted` (`crates/engine/src/gate/gate_tests.rs`) |
| A cancellation is logged | `a_dropped_question_is_logged_as_cancelled` (`crates/engine/src/gate/gate_tests.rs`), `a_cancelled_call_withdraws_its_question` (`crates/mcp/tests/it/cancel.rs`) |
| The log cannot be flooded | `the_log_keeps_the_newest_thousand` (`crates/store/src/agent_tests.rs`), `an_agent_cannot_flood_the_log_through_the_link` (`crates/engine/src/agent_link_tests.rs`) |
| A click does not answer the next question | `a_new_prompt_is_not_answered_by_the_click_for_the_old_one` (`crates/ui-egui/tests/session.rs`), `an_old_prompt_id_does_not_answer_the_next` (`crates/engine/src/gate/gate_tests.rs`), `tab_then_enter_or_space_never_answers_an_agent_question` (`crates/ui-egui/tests/agents.rs`) |
| Sessions end with the agent | `a_gone_client_withdraws_its_question_and_loses_its_sessions` (`crates/engine/src/gate/gate_tests.rs`), `closing_the_link_ends_sessions_and_withdraws_questions` (`crates/engine/src/agent_link_tests.rs`), `a_gone_mcp_process_ends_its_sessions_and_questions` (`crates/mcp/tests/it/cancel.rs`) |
