//! iOS 標準カレンダー (EventKit) 読み取りブリッジ (M20 データ連携 Part 2).

#[cfg(all(feature = "secure-vault", target_vendor = "apple"))]
pub mod commands;
#[cfg(all(feature = "secure-vault", target_vendor = "apple"))]
pub mod event_kit;
