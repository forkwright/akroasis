//! Yaesu FTM-510DR radio programming support.
//!
//! # Provenance
//!
//! The memory layout and channel structure here are derived from CHIRP's
//! Yaesu drivers, which are **GPL-3.0-or-later**: Copyright 2010 Dan Smith,
//! Copyright 2017 Wade Simmons (`chirp/drivers/yaesu_clone.py`,
//! `ftm3200d.py`, `ftm7250d.py`, each granting "either version 3 of the
//! License, or (at your option) any later version").
//!
//! Akroasis is AGPL-3.0, which satisfies that obligation — GPLv3 §13 permits
//! combining a covered work with an AGPLv3 work, and the combination is
//! governed by the AGPL's terms.
//!
//! WARNING: every derived item in this module points here rather than
//! restating the licence, and must keep doing so. Two rules hold it: do not
//! copy the licence name down to a derived site, and do not write it from
//! memory. An over-permissive label is the dangerous error — Apache-2.0, for
//! instance, grants patent and sublicensing rights GPL-3.0 withholds, so a
//! reader who trusts it takes permissions CHIRP's authors never gave, and
//! the label is what misled them. A licence repeated across four sites is a
//! licence that can be wrong in three of them silently.
//!
//! # Status
//!
//! Scaffolded. The clone-mode serial protocol has not been reverse-engineered
//! yet (requires ADMS-14 USB traffic capture).
//!
//! See forkwright/akroasis#80 for tracking.

pub mod codec;
pub mod protocol;
pub mod variant;
