use super::{Provider, Surface};

pub struct Gemini;
pub static PROVIDER: Gemini = Gemini;

impl Provider for Gemini {
    fn surface(&self) -> Surface {
        Surface::Listed
    }
    fn vendor(&self) -> Option<&'static str> {
        Some("google")
    }
    fn brands(&self) -> &'static [&'static str] {
        &["gemini"]
    }
    fn acp(&self) -> Option<&'static str> {
        Some("gemini-acp")
    }
}
