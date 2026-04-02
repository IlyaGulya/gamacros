pub mod binding;
pub mod effect;
pub mod padjutsu;
pub mod stick;

pub use effect::Effect;
pub use padjutsu::Padjutsu;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ButtonPhase {
    Pressed,
    Released,
}
