#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Theme {
    Dark,
    Light,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    pub theme: Theme,
    pub telemetry_opt_in: bool,
    pub reduce_motion: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: Theme::Dark,
            telemetry_opt_in: false,
            reduce_motion: false,
        }
    }
}

impl Settings {
    pub fn set(&mut self, key: &str, value: &str) -> Result<(), String> {
        match key {
            "theme" => {
                self.theme = match value {
                    "dark" => Theme::Dark,
                    "light" => Theme::Light,
                    _ => return Err(format!("unknown theme {value}")),
                };
            }
            "telemetry_opt_in" => {
                self.telemetry_opt_in = value == "true";
            }
            "reduce_motion" => {
                self.reduce_motion = value == "true";
            }
            _ => return Err(format!("unknown setting {key}")),
        }
        Ok(())
    }

    pub fn get(&self, key: &str) -> Option<String> {
        match key {
            "theme" => Some(
                match self.theme {
                    Theme::Dark => "dark",
                    Theme::Light => "light",
                }
                .to_string(),
            ),
            "telemetry_opt_in" => Some(self.telemetry_opt_in.to_string()),
            "reduce_motion" => Some(self.reduce_motion.to_string()),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_private() {
        let settings = Settings::default();
        assert_eq!(settings.theme, Theme::Dark);
        assert!(!settings.telemetry_opt_in);
    }
}
