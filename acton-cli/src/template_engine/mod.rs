use anyhow::{Context, Result};
use minijinja::Environment;
use rust_embed::Embed;
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

/// Embed all templates at compile time
#[derive(Embed)]
#[folder = "templates/"]
#[prefix = ""]
struct EmbeddedTemplates;

/// Template engine with support for embedded templates and user customization
pub struct TemplateEngine {
    config_dir: Option<PathBuf>,
}

impl TemplateEngine {
    /// Create a new template engine
    pub fn new() -> Result<Self> {
        // Get XDG config directory for user templates
        let config_dir = Self::get_config_dir();

        Ok(Self { config_dir })
    }

    /// Get the XDG config directory for acton-cli templates
    fn get_config_dir() -> Option<PathBuf> {
        directories::ProjectDirs::from("com", "govcraft", "acton-cli")
            .map(|proj_dirs| proj_dirs.config_dir().join("templates"))
    }

    /// Initialize user template directory with default templates
    pub fn init_user_templates(&self) -> Result<PathBuf> {
        let config_dir = self.config_dir.as_ref()
            .context("Could not determine config directory")?;

        // Create config directory if it doesn't exist
        fs::create_dir_all(config_dir)
            .context("Failed to create config directory")?;

        // Copy all embedded templates to user config directory
        for file_path in EmbeddedTemplates::iter() {
            let dest_path = config_dir.join(file_path.as_ref());

            // Create parent directories
            if let Some(parent) = dest_path.parent() {
                fs::create_dir_all(parent)
                    .context(format!("Failed to create directory: {}", parent.display()))?;
            }

            // Only copy if file doesn't exist (don't overwrite user customizations)
            if !dest_path.exists() {
                if let Some(file) = EmbeddedTemplates::get(&file_path) {
                    fs::write(&dest_path, file.data.as_ref())
                        .context(format!("Failed to write template: {}", dest_path.display()))?;
                }
            }
        }

        Ok(config_dir.clone())
    }

    /// Render a template with the given context
    pub fn render<S: Serialize>(&self, template_name: &str, context: &S) -> Result<String> {
        // Check if user has a custom template first
        let template_content = if let Some(ref config_dir) = self.config_dir {
            let user_template_path = config_dir.join(template_name);
            if user_template_path.exists() {
                fs::read_to_string(&user_template_path)
                    .context(format!("Failed to read user template: {}", user_template_path.display()))?
            } else {
                self.get_embedded_template(template_name)?
            }
        } else {
            self.get_embedded_template(template_name)?
        };

        // Create a fresh environment and render
        let env = Environment::new();
        let tmpl = env.template_from_str(&template_content)
            .context(format!("Failed to parse template: {}", template_name))?;

        let result = tmpl.render(context)
            .context("Failed to render template")?;

        Ok(result)
    }

    /// Get embedded template content
    fn get_embedded_template(&self, template_name: &str) -> Result<String> {
        let file = EmbeddedTemplates::get(template_name)
            .context(format!("Template not found: {}", template_name))?;

        let content = std::str::from_utf8(file.data.as_ref())
            .context("Failed to read embedded template as UTF-8")?;

        Ok(content.to_string())
    }

    /// Get the user config directory path (if it exists)
    pub fn config_dir(&self) -> Option<&Path> {
        self.config_dir.as_deref()
    }

    /// List all available templates
    pub fn list_templates(&self) -> Vec<String> {
        EmbeddedTemplates::iter()
            .map(|s| s.to_string())
            .collect()
    }
}

impl Default for TemplateEngine {
    fn default() -> Self {
        Self::new().expect("Failed to create template engine")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_template_engine_creation() {
        let engine = TemplateEngine::new();
        assert!(engine.is_ok());
    }

    #[test]
    fn test_list_templates() {
        let engine = TemplateEngine::new().unwrap();
        let templates = engine.list_templates();
        assert!(!templates.is_empty());
    }

    #[test]
    fn test_render_template() {
        let engine = TemplateEngine::new().unwrap();

        // Test with a simple context
        let context = json!({
            "name": "test-service",
            "http": true,
            "grpc": false,
        });

        // Try to render main.rs template
        let result = engine.render("service/main.rs.jinja", &context);
        assert!(result.is_ok());
    }
}
