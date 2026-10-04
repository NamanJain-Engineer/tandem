#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommandSpec {
    pub id: &'static str,
    pub title_key: &'static str,
    pub category: &'static str,
    pub default_shortcut: Option<&'static str>,
    pub enabled_predicate: &'static str,
}

pub fn builtin_commands() -> &'static [CommandSpec] {
    &[
        CommandSpec {
            id: "palette.open",
            title_key: "cmd.palette.open",
            category: "general",
            default_shortcut: Some("Mod+K"),
            enabled_predicate: "always",
        },
        CommandSpec {
            id: "sidebar.toggle",
            title_key: "cmd.sidebar.toggle",
            category: "view",
            default_shortcut: Some("Mod+B"),
            enabled_predicate: "always",
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique() {
        let cmds = builtin_commands();
        let mut ids: Vec<_> = cmds.iter().map(|cmd| cmd.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), cmds.len());
    }
}
