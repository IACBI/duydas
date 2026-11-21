//! Channel and community permissions.
//!
//! Resolution is deny-by-default: a member has exactly the union of their roles' grants, and
//! [`Permissions::ADMINISTRATOR`] implies every permission. Channel-level overwrites arrive in
//! Phase 2 (see `docs/architecture.md`).

use bitflags::bitflags;

bitflags! {
    /// A set of permissions. The bit values are part of the wire protocol: never reorder them.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    pub struct Permissions: u64 {
        /// See a channel and read its history.
        const VIEW_CHANNEL = 1 << 0;
        /// Post messages in text channels.
        const SEND_MESSAGES = 1 << 1;
        /// Delete or pin other members' messages.
        const MANAGE_MESSAGES = 1 << 2;
        /// Join voice channels.
        const CONNECT = 1 << 3;
        /// Transmit audio in voice channels.
        const SPEAK = 1 << 4;
        /// Create, edit and delete channels.
        const MANAGE_CHANNELS = 1 << 5;
        /// Create and assign roles below one's own highest role.
        const MANAGE_ROLES = 1 << 6;
        /// Remove members from the community.
        const KICK_MEMBERS = 1 << 7;
        /// Ban members from the community.
        const BAN_MEMBERS = 1 << 8;
        /// Implies every permission.
        const ADMINISTRATOR = 1 << 63;
    }
}

impl Permissions {
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
}
