//! Names for text fields, for screen readers. A field with a label beside it names itself through
//! `labelled_by`; a field that shows only a hint (the host search, a filter) would otherwise have
//! no name at all, so a screen reader could only say that a field is there. egui puts the hint
//! into the tree as the placeholder, which neither Android's adapter nor every desktop screen
//! reader reads as a name, so the placeholder becomes the name of such a field.

use egui::accesskit::{Role, TreeUpdate};

/// Fills in the names (see the module) in each frame's tree.
pub struct FieldNames;

impl egui::Plugin for FieldNames {
    fn debug_name(&self) -> &'static str {
        "sshcraft_field_names"
    }

    fn output_hook(&mut self, _ctx: &egui::Context, output: &mut egui::FullOutput) {
        if let Some(update) = &mut output.platform_output.accesskit_update {
            name_fields(update);
        }
    }
}

/// Names the field of `response` for screen readers where no label beside it can: a dialog
/// whose title says what the field is for, a field in a row of buttons.
pub fn name(ui: &egui::Ui, response: &egui::Response, name: &str) {
    ui.ctx().accesskit_node_builder(response.id, |node| node.set_label(name));
}

/// Whether `role` is a field the user types into.
pub fn is_text_field(role: Role) -> bool {
    matches!(role, Role::TextInput | Role::PasswordInput | Role::MultilineTextInput | Role::SearchInput)
}

/// Gives every text field in `update` without a name of its own its placeholder as the name.
/// A field named by a label (`labelled_by`) or by itself keeps that name.
pub fn name_fields(update: &mut TreeUpdate) {
    for (_, node) in &mut update.nodes {
        if is_text_field(node.role())
            && node.label().is_none()
            && node.labelled_by().is_empty()
            && let Some(placeholder) = node.placeholder().map(str::to_owned)
            && !placeholder.trim().is_empty()
        {
            node.set_label(placeholder);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::accesskit::{Node, NodeId, TreeId, TreeInfo};

    fn update(nodes: Vec<(u64, Node)>) -> TreeUpdate {
        TreeUpdate {
            nodes: nodes.into_iter().map(|(id, node)| (NodeId(id), node)).collect(),
            tree: Some(TreeInfo::new(NodeId(1))),
            tree_id: TreeId::ROOT,
            focus: NodeId(1),
        }
    }

    fn field(role: Role, placeholder: Option<&str>) -> Node {
        let mut node = Node::new(role);
        if let Some(text) = placeholder {
            node.set_placeholder(text);
        }
        node
    }

    #[test]
    fn a_field_with_only_a_hint_is_named_by_it() {
        let mut tree = update(vec![(2, field(Role::TextInput, Some("Search hosts"))), (3, field(Role::PasswordInput, Some("optional")))]);
        name_fields(&mut tree);
        assert_eq!(tree.nodes[0].1.label(), Some("Search hosts"));
        assert_eq!(tree.nodes[1].1.label(), Some("optional"));
    }

    #[test]
    fn a_named_field_keeps_its_name() {
        let mut labelled = field(Role::TextInput, Some("comma separated"));
        labelled.push_labelled_by(NodeId(9));
        let mut own = field(Role::TextInput, Some("hint"));
        own.set_label("Address");
        let mut tree = update(vec![(2, labelled), (3, own), (4, field(Role::Button, Some("not a field"))), (5, field(Role::TextInput, Some("  ")))]);
        name_fields(&mut tree);
        assert_eq!(tree.nodes[0].1.label(), None, "labelled_by names it");
        assert_eq!(tree.nodes[1].1.label(), Some("Address"));
        assert_eq!(tree.nodes[2].1.label(), None, "only fields");
        assert_eq!(tree.nodes[3].1.label(), None, "a blank hint is no name");
    }
}
