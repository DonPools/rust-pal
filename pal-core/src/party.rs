//! Platform-independent party membership and role attributes.

use pal_assets::player_roles::{PlayerRole, PlayerRoles, PLAYER_ROLE_COUNT};

pub const MAX_PARTY_MEMBERS: usize = 5;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartyMember {
    pub role_id: u16,
    pub attributes: PlayerRole,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Party {
    members: Vec<PartyMember>,
}

impl Party {
    pub fn single(role_id: u16, roles: &PlayerRoles) -> Option<Self> {
        let attributes = roles.role(usize::from(role_id))?.clone();
        Some(Self {
            members: vec![PartyMember {
                role_id,
                attributes,
            }],
        })
    }

    pub fn members(&self) -> &[PartyMember] {
        &self.members
    }

    pub fn leader(&self) -> Option<&PartyMember> {
        self.members.first()
    }

    /// Replace the active party from zero-based role IDs.
    pub fn replace(&mut self, role_ids: &[u16], roles: &PlayerRoles) -> bool {
        if role_ids.is_empty() || role_ids.len() > MAX_PARTY_MEMBERS {
            return false;
        }

        let mut replacement = Vec::with_capacity(role_ids.len());
        for &role_id in role_ids {
            if replacement
                .iter()
                .any(|member: &PartyMember| member.role_id == role_id)
            {
                return false;
            }
            let Some(attributes) = roles.role(usize::from(role_id)).cloned() else {
                return false;
            };
            replacement.push(PartyMember {
                role_id,
                attributes,
            });
        }

        self.members = replacement;
        true
    }

    pub fn add(&mut self, role_id: u16, roles: &PlayerRoles) -> bool {
        if self.members.len() >= MAX_PARTY_MEMBERS
            || usize::from(role_id) >= PLAYER_ROLE_COUNT
            || self.members.iter().any(|member| member.role_id == role_id)
        {
            return false;
        }
        let Some(attributes) = roles.role(usize::from(role_id)).cloned() else {
            return false;
        };
        self.members.push(PartyMember {
            role_id,
            attributes,
        });
        true
    }

    pub fn remove(&mut self, role_id: u16) -> bool {
        let Some(index) = self
            .members
            .iter()
            .position(|member| member.role_id == role_id)
        else {
            return false;
        };
        self.members.remove(index);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roles() -> PlayerRoles {
        PlayerRoles::parse(&vec![0; 900]).unwrap()
    }

    #[test]
    fn party_enforces_unique_members_and_capacity() {
        let roles = roles();
        let mut party = Party::single(0, &roles).unwrap();
        assert!(!party.add(0, &roles));
        for role in 1..MAX_PARTY_MEMBERS as u16 {
            assert!(party.add(role, &roles));
        }
        assert!(!party.add(MAX_PARTY_MEMBERS as u16, &roles));
        assert_eq!(party.members().len(), MAX_PARTY_MEMBERS);
        assert_eq!(party.leader().unwrap().role_id, 0);
    }

    #[test]
    fn removes_existing_members_only() {
        let roles = roles();
        let mut party = Party::single(0, &roles).unwrap();
        assert!(party.add(1, &roles));
        assert!(party.remove(0));
        assert_eq!(party.leader().unwrap().role_id, 1);
        assert!(!party.remove(0));
    }

    #[test]
    fn replaces_members_atomically() {
        let roles = roles();
        let mut party = Party::single(0, &roles).unwrap();
        assert!(party.replace(&[2, 1], &roles));
        assert_eq!(
            party
                .members()
                .iter()
                .map(|member| member.role_id)
                .collect::<Vec<_>>(),
            vec![2, 1]
        );

        assert!(!party.replace(&[2, 2], &roles));
        assert_eq!(party.members().len(), 2);
        assert_eq!(party.leader().unwrap().role_id, 2);
        assert!(!party.replace(&[], &roles));
    }
}
