pub mod binding;
pub mod effect;
pub mod gamacros;
pub mod stick;

pub use effect::Effect;
pub use gamacros::Gamacros;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ButtonPhase {
    Pressed,
    Released,
}
