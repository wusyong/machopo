//! A realm's `crypto` (WebCrypto's `Crypto`, Blink's `modules/crypto`):
//! its execution context's, the same object each time; what it does is
//! `dom_api`'s.

use elise::{Gc, GcObject, JsEdge};

use crate::{Cx, ExecutionContext, sites};

#[derive(GcObject)]
pub struct Crypto {
    wrapper: JsEdge,
}

impl<'gc> Cx<'_, 'gc> {
    /// `context`'s `crypto`, the same each time.
    pub fn context_crypto(&self, context: Gc<'gc, ExecutionContext>) -> Gc<'gc, Crypto> {
        if let Some(crypto) = context.crypto.get(self.gc) {
            return crypto;
        }
        let crypto = self.gc.alloc_in(&sites::OTHER, CryptoInit { wrapper: () });
        context.crypto.set(self.gc, Some(crypto));
        crypto
    }

    pub(crate) fn crypto_wrapper_edge(&self, crypto: Gc<'gc, Crypto>) -> &'gc JsEdge {
        &crypto.into_ref().wrapper
    }
}
