use std::sync::Arc;

use gamacros_workspace::{ButtonRules, Profile, StickRules};

use super::stick::CompiledStickRules;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BindingSource {
    #[default]
    None,
    App,
    Common,
    Blacklisted,
}

#[derive(Debug, Clone, Default)]
pub struct BindingContext {
    active_app: Box<str>,
    source: BindingSource,
    button_rules: Option<Arc<ButtonRules>>,
    stick_rules: Option<Arc<StickRules>>,
    compiled_stick_rules: Option<CompiledStickRules>,
    shell: Option<Box<str>>,
}

impl BindingContext {
    pub fn rebuild(profile: Option<&Profile>, active_app: &str) -> Self {
        let Some(profile) = profile else {
            return Self::empty(active_app);
        };

        if !active_app.is_empty() && profile.blacklist.contains(active_app) {
            return Self {
                active_app: active_app.into(),
                source: BindingSource::Blacklisted,
                button_rules: None,
                stick_rules: None,
                compiled_stick_rules: None,
                shell: profile.shell.clone(),
            };
        }

        let (source, rules) = match profile.rules.get(active_app) {
            Some(app_rules) if !active_app.is_empty() => {
                (BindingSource::App, Some(app_rules))
            }
            _ => match profile.rules.get("common") {
                Some(common_rules) => (BindingSource::Common, Some(common_rules)),
                None => (BindingSource::None, None),
            },
        };

        let button_rules = rules.map(|rules| Arc::new(rules.buttons.clone()));
        let stick_rules = rules.map(|rules| Arc::new(rules.sticks.clone()));
        let compiled_stick_rules =
            stick_rules.as_deref().map(CompiledStickRules::from_rules);

        Self {
            active_app: active_app.into(),
            source,
            button_rules,
            stick_rules,
            compiled_stick_rules,
            shell: profile.shell.clone(),
        }
    }

    pub fn empty(active_app: &str) -> Self {
        Self {
            active_app: active_app.into(),
            source: BindingSource::None,
            button_rules: None,
            stick_rules: None,
            compiled_stick_rules: None,
            shell: None,
        }
    }

    pub fn active_app(&self) -> &str {
        &self.active_app
    }

    pub fn source(&self) -> BindingSource {
        self.source
    }

    pub fn is_blacklisted(&self) -> bool {
        self.source == BindingSource::Blacklisted
    }

    pub fn button_rules(&self) -> Option<&ButtonRules> {
        self.button_rules.as_deref()
    }

    pub fn stick_rules(&self) -> Option<&StickRules> {
        self.stick_rules.as_deref()
    }

    pub fn compiled_stick_rules(&self) -> Option<&CompiledStickRules> {
        self.compiled_stick_rules.as_ref()
    }

    pub fn shell(&self) -> Option<&str> {
        self.shell.as_deref()
    }
}
