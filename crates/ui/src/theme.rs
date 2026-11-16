//! Themes as tables of named tokens (design tokens; CSS's custom
//! properties). What the tokens are is the app's: it declares them with
//! [`tokens!`](crate::tokens), and builds each theme's table with a `match`
//! over them, so a token without a value doesn't compile. Reading one is an
//! array index.

use std::marker::PhantomData;

/// An enum of tokens, declared with [`tokens!`](crate::tokens).
pub trait Token: Copy + 'static {
    /// Every token, in order: `ALL[t.index()] == t`.
    const ALL: &'static [Self];
    fn index(self) -> usize;
    /// The token's name, as a theme file or a style sheet would have it.
    fn name(self) -> &'static str;
}

/// A value for every token of `K`.
#[derive(Debug, Clone, PartialEq)]
pub struct TokenTable<K: Token, V> {
    values: Box<[V]>,
    _tokens: PhantomData<fn() -> K>,
}

impl<K: Token, V> TokenTable<K, V> {
    /// The table `value` gives: a `match` over the tokens, so that a new
    /// token has to get a value in every theme.
    pub fn new(value: impl Fn(K) -> V) -> Self {
        Self {
            values: K::ALL.iter().map(|&token| value(token)).collect(),
            _tokens: PhantomData,
        }
    }

    pub fn get(&self, token: K) -> &V {
        &self.values[token.index()]
    }

    /// Gives `token` another value (a theme file's, say).
    pub fn set(&mut self, token: K, value: V) {
        self.values[token.index()] = value;
    }
}

/// Declares an enum of theme tokens and implements [`Token`] for it:
///
/// ```ignore
/// ui::tokens! {
///     /// The browser's colors.
///     pub enum ColorToken {
///         ToolbarBackground,
///         Icon,
///     }
/// }
/// ```
#[macro_export]
macro_rules! tokens {
    ($(#[$meta:meta])* $vis:vis enum $name:ident { $($(#[$vmeta:meta])* $variant:ident),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        $vis enum $name {
            $($(#[$vmeta])* $variant),+
        }

        impl $crate::Token for $name {
            const ALL: &'static [Self] = &[$($name::$variant),+];

            fn index(self) -> usize {
                // A fieldless enum's discriminant, from 0 in declaration
                // order: its index in `ALL`.
                self as usize
            }

            fn name(self) -> &'static str {
                match self {
                    $($name::$variant => stringify!($variant)),+
                }
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    crate::tokens! {
        enum Shade {
            Light,
            Middle,
            Dark,
        }
    }

    #[test]
    fn a_table_has_a_value_for_each_token_in_order() {
        assert_eq!(Shade::ALL, [Shade::Light, Shade::Middle, Shade::Dark]);
        for (i, shade) in Shade::ALL.iter().enumerate() {
            assert_eq!(shade.index(), i);
        }
        let mut table = TokenTable::new(|shade| match shade {
            Shade::Light => 10,
            Shade::Middle => 20,
            Shade::Dark => 30,
        });
        assert_eq!(*table.get(Shade::Middle), 20);
        assert_eq!(*table.get(Shade::Dark), 30);
        table.set(Shade::Middle, 25);
        assert_eq!(*table.get(Shade::Middle), 25);
        assert_eq!(*table.get(Shade::Light), 10);
        assert_eq!(Shade::Dark.name(), "Dark");
    }
}
