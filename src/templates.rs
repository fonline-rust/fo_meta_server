use custom_error::custom_error;
use lazy_static::lazy_static;
use serde::Serialize;
use tera::Tera;

use crate::config::Host;

const TEMPLATES_PATH: &str = "templates/**/*";
const CSS_PATH: &str = "static/charsheet.css";
const SCSS_PATH: &str = "styles/charsheet.scss";

struct Templates {
    tera: Tera,
    css: String,
}

custom_error! {pub TemplatesError
    Io{source: std::io::Error}     = "fs error",
    Tera{source: tera::Error}      = "tera error",
    Rsass{inner: rsass::Error}     = "rsass error",
    Utf8{source: std::string::FromUtf8Error} = "string conversion error"
}

impl Templates {
    fn new() -> Self {
        let css = Self::compile_css().unwrap();
        let tera = Tera::new(TEMPLATES_PATH).unwrap();
        let templates = Templates { tera, css };
        templates.write_css().unwrap();
        templates
    }

    #[cfg(feature = "live_reload")]
    fn remake(&mut self) -> Result<(), TemplatesError> {
        self.css = Self::compile_css()?;
        self.tera.full_reload()?;
        self.write_css()?;
        Ok(())
    }

    fn write_css(&self) -> std::io::Result<()> {
        std::fs::write(CSS_PATH, &self.css)
    }

    fn compile_css() -> Result<String, TemplatesError> {
        let vec = rsass::compile_scss_path(SCSS_PATH.as_ref(), Default::default())
            .map_err(|err| TemplatesError::Rsass { inner: err })?;
        let string = String::from_utf8(vec)?;
        //OutputStyle::Compressed
        Ok(string)
    }
}

#[cfg(not(feature = "live_reload"))]
lazy_static! {
    static ref TEMPLATES: Templates = Templates::new();
}

#[cfg(feature = "live_reload")]
lazy_static! {
    static ref TEMPLATES: std::sync::Mutex<Templates> = {
        let mut templates = Templates::new();
        std::sync::Mutex::new(templates)
    };
}

#[derive(Debug, Default)]
pub struct RenderConfig<'a> {
    pub host: Option<&'a Host>,
}

#[cfg(not(feature = "live_reload"))]
pub fn render<T: Serialize>(
    template: &str,
    data: &T,
    config: RenderConfig,
) -> Result<String, TemplatesError> {
    let mut context = tera::Context::from_serialize(data)?;
    if let Some(host) = config.host {
        context.insert("files_url", &host.web_url(""));
    }
    Ok(TEMPLATES.tera.render(template, &context)?)
}
#[cfg(feature = "live_reload")]
pub fn render<T: Serialize>(
    template: &str,
    data: &T,
    config: RenderConfig,
) -> Result<String, TemplatesError> {
    let mut templates = TEMPLATES.lock().unwrap();
    let mut context = tera::Context::from_serialize(data)?;
    if let Some(host) = config.host {
        context.insert("files_url", &host.web_url(""));
    }
    templates.remake()?;
    Ok(templates.tera.render(template, &context)?)
}

pub fn init() {
    let _ = *TEMPLATES;
}
