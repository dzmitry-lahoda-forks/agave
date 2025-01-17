//! History of stake activations and de-activations.
//!
//! The _stake history sysvar_ provides access to the [`StakeHistory`] type.
//!
//! The [`Sysvar::get`] method always returns
//! [`ProgramError::UnsupportedSysvar`], and in practice the data size of this
//! sysvar is too large to process on chain. One can still use the
//! [`SysvarId::id`], [`SysvarId::check_id`] and [`Sysvar::size_of`] methods in
//! an on-chain program, and it can be accessed off-chain through RPC.
//!
//! [`ProgramError::UnsupportedSysvar`]: https://docs.rs/solana-program-error/latest/solana_program_error/enum.ProgramError.html#variant.UnsupportedSysvar
//! [`SysvarId::id`]: https://docs.rs/solana-sysvar-id/latest/solana_sysvar_id/trait.SysvarId.html
//! [`SysvarId::check_id`]: https://docs.rs/solana-sysvar-id/latest/solana_sysvar_id/trait.SysvarId.html#tymethod.check_id
//!
//! # Examples
//!
//! Calling via the RPC client:
//!
//! ```
//! # use solana_program::example_mocks::solana_sdk;
//! # use solana_program::example_mocks::solana_rpc_client;
//! # use solana_program::stake_history::StakeHistory;
//! # use solana_sdk::account::Account;
//! # use solana_rpc_client::rpc_client::RpcClient;
//! # use solana_sdk_ids::sysvar::stake_history;
//! # use anyhow::Result;
//! #
//! fn print_sysvar_stake_history(client: &RpcClient) -> Result<()> {
//! #   client.set_get_account_response(stake_history::ID, Account {
//! #       lamports: 114979200,
//! #       data: vec![0, 0, 0, 0, 0, 0, 0, 0],
//! #       owner: solana_sdk_ids::system_program::ID,
//! #       executable: false,
//! #       rent_epoch: 307,
//! #   });
//! #
//!     let stake_history = client.get_account(&stake_history::ID)?;
//!     let data: StakeHistory = bincode::deserialize(&stake_history.data)?;
//!
//!     Ok(())
//! }
//! #
//! # let client = RpcClient::new(String::new());
//! # print_sysvar_stake_history(&client)?;
//! #
//! # Ok::<(), anyhow::Error>(())
//! ```

#[cfg(feature = "serde")]
use serde_derive::{Deserialize, Serialize};
pub use solana_sdk_ids::sysvar::stake_history::{check_id, id, ID};
#[cfg(feature = "bincode")]
use {
    crate::{get_sysvar, Sysvar},
    solana_sysvar_id::SysvarId,
};
use {solana_clock::Epoch, solana_sysvar_id::impl_sysvar_id, std::ops::Deref};

pub const MAX_ENTRIES: usize = 512; // it should never take as many as 512 epochs to warm up or cool down

#[repr(C)]
#[cfg_attr(feature = "frozen-abi", derive(solana_frozen_abi_macro::AbiExample))]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
#[derive(Debug, PartialEq, Eq, Default, Clone)]
pub struct StakeHistoryEntry {
    pub effective: u64,    // effective stake at this epoch
    pub activating: u64,   // sum of portion of stakes not fully warmed up
    pub deactivating: u64, // requested to be cooled down, not fully deactivated yet
}

impl StakeHistoryEntry {
    pub fn with_effective(effective: u64) -> Self {
        Self {
            effective,
            ..Self::default()
        }
    }

    pub fn with_effective_and_activating(effective: u64, activating: u64) -> Self {
        Self {
            effective,
            activating,
            ..Self::default()
        }
    }

    pub fn with_deactivating(deactivating: u64) -> Self {
        Self {
            effective: deactivating,
            deactivating,
            ..Self::default()
        }
    }
}

impl std::ops::Add for StakeHistoryEntry {
    type Output = StakeHistoryEntry;
    fn add(self, rhs: StakeHistoryEntry) -> Self::Output {
        Self {
            effective: self.effective.saturating_add(rhs.effective),
            activating: self.activating.saturating_add(rhs.activating),
            deactivating: self.deactivating.saturating_add(rhs.deactivating),
        }
    }
}

/// A type to hold data for the [`StakeHistory` sysvar][sv].
///
/// [sv]: https://docs.solanalabs.com/runtime/sysvars#stakehistory
#[repr(C)]
#[cfg_attr(feature = "frozen-abi", derive(solana_frozen_abi_macro::AbiExample))]
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
#[derive(Debug, PartialEq, Eq, Default, Clone)]
pub struct StakeHistory(Vec<(Epoch, StakeHistoryEntry)>);

impl StakeHistory {
    pub fn get(&self, epoch: Epoch) -> Option<&StakeHistoryEntry> {
        self.binary_search_by(|probe| epoch.cmp(&probe.0))
            .ok()
            .map(|index| &self[index].1)
    }

    pub fn add(&mut self, epoch: Epoch, entry: StakeHistoryEntry) {
        match self.binary_search_by(|probe| epoch.cmp(&probe.0)) {
            Ok(index) => (self.0)[index] = (epoch, entry),
            Err(index) => (self.0).insert(index, (epoch, entry)),
        }
        (self.0).truncate(MAX_ENTRIES);
    }
}

impl Deref for StakeHistory {
    type Target = Vec<(Epoch, StakeHistoryEntry)>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

pub trait StakeHistoryGetEntry {
    fn get_entry(&self, epoch: Epoch) -> Option<StakeHistoryEntry>;
}

impl StakeHistoryGetEntry for StakeHistory {
    fn get_entry(&self, epoch: Epoch) -> Option<StakeHistoryEntry> {
        self.binary_search_by(|probe| epoch.cmp(&probe.0))
            .ok()
            .map(|index| self[index].1.clone())
    }
}

impl_sysvar_id!(StakeHistory);

#[cfg(feature = "bincode")]
impl Sysvar for StakeHistory {
    // override
    fn size_of() -> usize {
        // hard-coded so that we don't have to construct an empty
        16392 // golden, update if MAX_ENTRIES changes
    }
}

// we do not provide Default because this requires the real current epoch
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct StakeHistorySysvar(pub Epoch);

// precompute so we can statically allocate buffer
#[cfg(feature = "bincode")]
const EPOCH_AND_ENTRY_SERIALIZED_SIZE: u64 = 32;

#[cfg(feature = "bincode")]
impl StakeHistoryGetEntry for StakeHistorySysvar {
    fn get_entry(&self, target_epoch: Epoch) -> Option<StakeHistoryEntry> {
        let current_epoch = self.0;

        // if current epoch is zero this returns None because there is no history yet
        let newest_historical_epoch = current_epoch.checked_sub(1)?;
        let oldest_historical_epoch = current_epoch.saturating_sub(MAX_ENTRIES as u64);

        // target epoch is old enough to have fallen off history; presume fully active/deactive
        if target_epoch < oldest_historical_epoch {
            return None;
        }

        // epoch delta is how many epoch-entries we offset in the stake history vector, which may be zero
        // None means target epoch is current or in the future; this is a user error
        let epoch_delta = newest_historical_epoch.checked_sub(target_epoch)?;

        // offset is the number of bytes to our desired entry, including eight for vector length
        let offset = epoch_delta
            .checked_mul(EPOCH_AND_ENTRY_SERIALIZED_SIZE)?
            .checked_add(std::mem::size_of::<u64>() as u64)?;

        let mut entry_buf = [0; EPOCH_AND_ENTRY_SERIALIZED_SIZE as usize];
        let result = get_sysvar(
            &mut entry_buf,
            &StakeHistory::id(),
            offset,
            EPOCH_AND_ENTRY_SERIALIZED_SIZE,
        );

        match result {
            Ok(()) => {
                let (entry_epoch, entry) =
                    bincode::deserialize::<(Epoch, StakeHistoryEntry)>(&entry_buf).ok()?;

                // this would only fail if stake history skipped an epoch or the binary format of the sysvar changed
                assert_eq!(entry_epoch, target_epoch);

                Some(entry)
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use {super::*, crate::tests::mock_get_sysvar_syscall, serial_test::serial};

    #[test]
    fn test_stake_history() {
        let mut stake_history = StakeHistory::default();

        for i in 0..MAX_ENTRIES as u64 + 1 {
            stake_history.add(
                i,
                StakeHistoryEntry {
                    activating: i,
                    ..StakeHistoryEntry::default()
                },
            );
        }
        assert_eq!(stake_history.len(), MAX_ENTRIES);
        assert_eq!(stake_history.iter().map(|entry| entry.0).min().unwrap(), 1);
        assert_eq!(stake_history.get(0), None);
        assert_eq!(
            stake_history.get(1),
            Some(&StakeHistoryEntry {
                activating: 1,
                ..StakeHistoryEntry::default()
            })
        );
    }

    #[test]
    fn test_size_of() {
        let mut stake_history = StakeHistory::default();
        for i in 0..MAX_ENTRIES as u64 {
            stake_history.add(
                i,
                StakeHistoryEntry {
                    activating: i,
                    ..StakeHistoryEntry::default()
                },
            );
        }

        assert_eq!(
            bincode::serialized_size(&stake_history).unwrap() as usize,
            StakeHistory::size_of()
        );

        let stake_history_inner: Vec<(Epoch, StakeHistoryEntry)> =
            bincode::deserialize(&bincode::serialize(&stake_history).unwrap()).unwrap();
        let epoch_entry = stake_history_inner.into_iter().next().unwrap();

        assert_eq!(
            bincode::serialized_size(&epoch_entry).unwrap(),
            EPOCH_AND_ENTRY_SERIALIZED_SIZE
        );
    }

    #[serial]
    #[test]
    fn test_stake_history_get_entry() {
        let unique_entry_for_epoch = |epoch: u64| StakeHistoryEntry {
            activating: epoch.saturating_mul(2),
            deactivating: epoch.saturating_mul(3),
            effective: epoch.saturating_mul(5),
        };

        let current_epoch = MAX_ENTRIES.saturating_add(2) as u64;

        // make a stake history object with at least one valid entry that has expired
        let mut stake_history = StakeHistory::default();
        for i in 0..current_epoch {
            stake_history.add(i, unique_entry_for_epoch(i));
        }
        assert_eq!(stake_history.len(), MAX_ENTRIES);
        assert_eq!(stake_history.iter().map(|entry| entry.0).min().unwrap(), 2);

        // set up sol_get_sysvar
        mock_get_sysvar_syscall(&bincode::serialize(&stake_history).unwrap());

        // make a syscall interface object
        let stake_history_sysvar = StakeHistorySysvar(current_epoch);

        // now test the stake history interfaces

        assert_eq!(stake_history.get(0), None);
        assert_eq!(stake_history.get(1), None);
        assert_eq!(stake_history.get(current_epoch), None);

        assert_eq!(stake_history.get_entry(0), None);
        assert_eq!(stake_history.get_entry(1), None);
        assert_eq!(stake_history.get_entry(current_epoch), None);

        assert_eq!(stake_history_sysvar.get_entry(0), None);
        assert_eq!(stake_history_sysvar.get_entry(1), None);
        assert_eq!(stake_history_sysvar.get_entry(current_epoch), None);

        for i in 2..current_epoch {
            let entry = Some(unique_entry_for_epoch(i));

            assert_eq!(stake_history.get(i), entry.as_ref(),);

            assert_eq!(stake_history.get_entry(i), entry,);

            assert_eq!(stake_history_sysvar.get_entry(i), entry,);
        }
    }

    #[serial]
    #[test]
    fn test_stake_history_get_entry_zero() {
        let mut current_epoch = 0;

        // first test that an empty history returns None
        let stake_history = StakeHistory::default();
        assert_eq!(stake_history.len(), 0);

        mock_get_sysvar_syscall(&bincode::serialize(&stake_history).unwrap());
        let stake_history_sysvar = StakeHistorySysvar(current_epoch);

        assert_eq!(stake_history.get(0), None);
        assert_eq!(stake_history.get_entry(0), None);
        assert_eq!(stake_history_sysvar.get_entry(0), None);

        // next test that we can get a zeroth entry in the first epoch
        let entry_zero = StakeHistoryEntry {
            effective: 100,
            ..StakeHistoryEntry::default()
        };
        let entry = Some(entry_zero.clone());

        let mut stake_history = StakeHistory::default();
        stake_history.add(current_epoch, entry_zero);
        assert_eq!(stake_history.len(), 1);
        current_epoch = current_epoch.saturating_add(1);

        mock_get_sysvar_syscall(&bincode::serialize(&stake_history).unwrap());
        let stake_history_sysvar = StakeHistorySysvar(current_epoch);

        assert_eq!(stake_history.get(0), entry.as_ref());
        assert_eq!(stake_history.get_entry(0), entry);
        assert_eq!(stake_history_sysvar.get_entry(0), entry);

        // finally test that we can still get a zeroth entry in later epochs
        stake_history.add(current_epoch, StakeHistoryEntry::default());
        assert_eq!(stake_history.len(), 2);
        current_epoch = current_epoch.saturating_add(1);

        mock_get_sysvar_syscall(&bincode::serialize(&stake_history).unwrap());
        let stake_history_sysvar = StakeHistorySysvar(current_epoch);

        assert_eq!(stake_history.get(0), entry.as_ref());
        assert_eq!(stake_history.get_entry(0), entry);
        assert_eq!(stake_history_sysvar.get_entry(0), entry);
    }
}
