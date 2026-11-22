//! Channel and community permissions.
//!
//! Resolution is deny-by-default: a member has exactly the union of their roles' grants, and
//! [`Permissions::ADMINISTRATOR`] implies every permission. Channel-level overwrites arrive in
//! Phase 2 (see `docs/architecture.md`).

use std::fmt;
use std::ops::{BitAnd, BitOr, BitOrAssign, Not, Sub};

/// A set of permissions. The bit values are part of the wire protocol: never reorder them.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Permissions(u64);

macro_rules! flags {
    ($($(#[$doc:meta])* $name:ident = $bit:expr;)*) => {
        impl Permissions {
            $($(#[$doc])* pub const $name: Permissions = Permissions(1 << $bit);)*

            /// Every defined permission, with names, in bit order.
            const NAMED: &'static [(&'static str, Permissions)] = &[$((stringify!($name), Permissions::$name)),*];
        }
    };
}

flags! {
    /// See a channel and read its history.
    VIEW_CHANNEL = 0;
    /// Post messages in text channels.
    SEND_MESSAGES = 1;
    /// Delete or pin other members' messages.
    MANAGE_MESSAGES = 2;
    /// Join voice channels.
    CONNECT = 3;
    /// Transmit audio in voice channels.
    SPEAK = 4;
    /// Create, edit and delete channels.
    MANAGE_CHANNELS = 5;
    /// Create and assign roles below one's own highest role.
    MANAGE_ROLES = 6;
    /// Remove members from the community.
    KICK_MEMBERS = 7;
    /// Ban members from the community.
    BAN_MEMBERS = 8;
    /// Implies every permission.
    ADMINISTRATOR = 63;
}

impl Permissions {
    /// No permissions.
    pub const fn empty() -> Self {
        Permissions(0)
    }

    /// Every defined permission (unknown bits excluded).
    pub const fn all() -> Self {
        let mut bits = 0;
        let mut i = 0;
        while i < Self::NAMED.len() {
            bits |= Self::NAMED[i].1.0;
            i += 1;
        }
        Permissions(bits)
    }

    /// The raw wire value.
    pub const fn bits(self) -> u64 {
        self.0
    }

    /// Parses a wire value, rejecting bits this version does not know.
    pub const fn from_bits(bits: u64) -> Option<Self> {
        if bits & !Self::all().0 == 0 {
            Some(Permissions(bits))
        } else {
            None
        }
    }

    /// Parses a wire value, dropping unknown bits (from a newer server).
    pub const fn from_bits_truncate(bits: u64) -> Self {
        Permissions(bits & Self::all().0)
    }

    /// Keeps every bit, known or not, e.g. to store and forward a newer server's value unchanged.
    pub const fn from_bits_retain(bits: u64) -> Self {
        Permissions(bits)
    }

    /// Adds the permissions in `other`.
    pub fn insert(&mut self, other: Self) {
        self.0 |= other.0;
    }

    /// Removes the permissions in `other`.
    pub fn remove(&mut self, other: Self) {
        self.0 &= !other.0;
    }

    /// Whether no permission is set.
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Whether every permission in `other` is set.
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Whether any permission in `other` is set.
    pub const fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }

    /// Effective permissions for a member holding `roles`.
    pub fn effective<'a>(roles: impl IntoIterator<Item = &'a Permissions>) -> Permissions {
        let granted = roles
            .into_iter()
            .fold(Permissions::empty(), |acc, role| acc | *role);
        if granted.contains(Permissions::ADMINISTRATOR) {
            Permissions::all()
        } else {
            granted
        }
    }

    /// Whether every permission in `required` is granted.
    pub fn allows(self, required: Permissions) -> bool {
        self.contains(required)
    }
}

impl BitOr for Permissions {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Permissions(self.0 | rhs.0)
    }
}

impl BitOrAssign for Permissions {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

impl BitAnd for Permissions {
    type Output = Self;
    fn bitand(self, rhs: Self) -> Self {
        Permissions(self.0 & rhs.0)
    }
}

impl Sub for Permissions {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        Permissions(self.0 & !rhs.0)
    }
}

impl Not for Permissions {
    type Output = Self;
    /// The complement within the defined permissions.
    fn not(self) -> Self {
        Permissions(!self.0 & Self::all().0)
    }
}

impl fmt::Debug for Permissions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Permissions(")?;
        let mut first = true;
        for (name, flag) in Self::NAMED {
            if self.contains(*flag) {
                if !first {
                    f.write_str(" | ")?;
                }
                f.write_str(name)?;
                first = false;
            }
        }
        let unknown = self.0 & !Self::all().0;
        if unknown != 0 {
            if !first {
                f.write_str(" | ")?;
            }
            write!(f, "{unknown:#x}")?;
        }
        f.write_str(")")
    }
}

#[cfg(test)]
mod tests {
    use super::Permissions as P;

    #[test]
    fn no_roles_means_no_permissions() {
        assert_eq!(P::effective([]), P::empty());
        assert!(!P::effective([]).allows(P::VIEW_CHANNEL));
    }

    #[test]
    fn roles_union() {
        let effective = P::effective(&[P::VIEW_CHANNEL, P::SEND_MESSAGES]);
        assert!(effective.allows(P::VIEW_CHANNEL | P::SEND_MESSAGES));
        assert!(!effective.allows(P::MANAGE_MESSAGES));
    }

    #[test]
    fn administrator_implies_all() {
        assert_eq!(P::effective(&[P::ADMINISTRATOR]), P::all());
    }

    #[test]
    fn wire_values_are_stable() {
        assert_eq!(P::VIEW_CHANNEL.bits(), 1);
        assert_eq!(P::BAN_MEMBERS.bits(), 1 << 8);
        assert_eq!(P::ADMINISTRATOR.bits(), 1 << 63);
        assert_eq!(P::all().bits(), 0x1ff | 1 << 63);
    }

    #[test]
    fn unknown_bits_from_newer_servers() {
        assert_eq!(P::from_bits(1 << 20), None);
        assert_eq!(P::from_bits_truncate(1 << 20 | 1), P::VIEW_CHANNEL);
        assert_eq!(P::from_bits(3), Some(P::VIEW_CHANNEL | P::SEND_MESSAGES));
        assert_eq!(P::from_bits_retain(1 << 20).bits(), 1 << 20);
        let mut p = P::VIEW_CHANNEL;
        p.insert(P::SPEAK);
        p.remove(P::VIEW_CHANNEL);
        assert_eq!(p, P::SPEAK);
    }

    #[test]
    fn set_operations_and_debug() {
        let both = P::VIEW_CHANNEL | P::SPEAK;
        assert_eq!(both - P::SPEAK, P::VIEW_CHANNEL);
        assert_eq!(both & P::SPEAK, P::SPEAK);
        assert!(!(!both).intersects(both));
        assert_eq!(format!("{both:?}"), "Permissions(VIEW_CHANNEL | SPEAK)");
        assert_eq!(format!("{:?}", P::empty()), "Permissions()");
    }
}
