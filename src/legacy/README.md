# This module contains a failed idea of improving Peritext

[Peritext is not agnostic to the underlying plain text CRDT](https://github.com/inkandswitch/peritext/issues/31).
I thought it's possible to build a useful lib that may add Peritext ability to a
existing list CRDT lib without changing its behaviors. But it turns out to be
too complicated to use and lack of the property my library need. The code is
also included in the crdt-richtext under the legacy module.

The initial motivation was to create a standalone module, decoupled from the
underlying list CRDT algorithm. This was successfully implemented, but the final
version was highly complex, posing significant integration challenges.

Furthermore, it lacked a crucial attribute I initially hoped it would possess:
the ability to compute version changes based purely on the operation sequence,
independent of the original state.

Ultimately, the additional overhead associated with this decoupled approach led
me to abandon this idea. The method required synchronization of many basic
operations on both sides, often involving similar calculations. This decoupled
approach didn't allow for simultaneous resolution of repeated calculations. For
instance, text insertion required updates to both the original text CRDT
document and the range CRDT length mapping.

Therefore, the value of this method became limited. Hence, it might be more
beneficial to develop a comprehensive rich-text from scratch, which we can
integrate to Loro CRDT more easily.
