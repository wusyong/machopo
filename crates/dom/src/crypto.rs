//! A window's `crypto` (WebCrypto's `Crypto`, Blink's `modules/crypto`):
//! the same object each time; what it does is `dom_api`'s.

use elise::{Gc, GcObject, JsEdge};

use crate::{Cx, Window, sites};

#[derive(GcObject)]
pub struct Crypto {
    wrapper: JsEdge,
}

impl<'gc> Cx<'_, 'gc> {
    /// `window`'s `crypto`, the same each time.
    pub fn window_crypto(&self, window: Gc<'gc, Window>) -> Gc<'gc, Crypto> {
        if let Some(crypto) = window.crypto.get(self.gc) {
            return crypto;
        }
        let crypto = self.gc.alloc_in(&sites::OTHER, CryptoInit { wrapper: () });
        window.crypto.set(self.gc, Some(crypto));
        crypto
    }

    pub(crate) fn crypto_wrapper_edge(&self, crypto: Gc<'gc, Crypto>) -> &'gc JsEdge {
        &crypto.into_ref().wrapper
    }
}
