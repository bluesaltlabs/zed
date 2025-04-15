use anyhow::Result;
use fs::Fs;
use serde_json::{Map, Value};

use std::sync::Arc;

pub struct VSCodeSettings {
    content: Map<String, Value>,
}

impl VSCodeSettings {
    pub fn from_str(content: &str) -> Result<Self> {
        Ok(Self {
            content: serde_json::from_str(content)?,
        })
    }

    pub async fn load_user_settings(fs: Arc<dyn Fs>) -> Result<Self> {
        let content = fs.load(paths::vscode_settings_file()).await?;
        Ok(Self {
            content: serde_json::from_str(&content)?,
        })
    }

    pub fn read_value(&self, setting: &str) -> Option<&Value> {
        if let Some(value) = self.content.get(setting) {
            return Some(value);
        }
        // TODO: check if it's in [platform] settings for current platform
        // TODO: deal with language specific settings
        None
    }

    pub fn read_string(&self, setting: &str) -> Option<&str> {
        self.content.get(setting).and_then(|v| v.as_str())
    }

    pub fn bool_setting(&self, key: &str, setting: &mut Option<bool>) {
        if let Some(s) = self.content.get(key).and_then(Value::as_bool) {
            *setting = Some(s)
        }
    }

    pub fn i32_setting(&self, key: &str, setting: &mut Option<i32>) {
        if let Some(s) = self.content.get(key).and_then(Value::as_i64) {
            *setting = Some(s as i32)
        }
    }

    pub fn i64_setting(&self, key: &str, setting: &mut Option<i64>) {
        if let Some(s) = self.content.get(key).and_then(Value::as_i64) {
            *setting = Some(s)
        }
    }

    pub fn u32_setting(&self, key: &str, setting: &mut Option<u32>) {
        if let Some(s) = self.content.get(key).and_then(Value::as_u64) {
            *setting = Some(s as u32)
        }
    }

    pub fn u64_setting(&self, key: &str, setting: &mut Option<u64>) {
        if let Some(s) = self.content.get(key).and_then(Value::as_u64) {
            *setting = Some(s)
        }
    }

    pub fn f32_setting(&self, key: &str, setting: &mut Option<f32>) {
        if let Some(s) = self.content.get(key).and_then(Value::as_f64) {
            *setting = Some(s as f32)
        }
    }

    pub fn f64_setting(&self, key: &str, setting: &mut Option<f64>) {
        if let Some(s) = self.content.get(key).and_then(Value::as_f64) {
            *setting = Some(s)
        }
    }

    pub fn enum_setting<T>(
        &self,
        key: &str,
        setting: &mut Option<T>,
        f: impl FnOnce(&str) -> Option<T>,
    ) {
        if let Some(s) = self.content.get(key).and_then(Value::as_str).and_then(f) {
            *setting = Some(s)
        }
    }
}
