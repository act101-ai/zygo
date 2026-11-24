# MS-4 — Address grammar and the ULID file form

**Spec:** `docs/specs/2026-11-24-mail-sync-design.md` S§5 (Address, Local address), S§7.2, S§8.5 "Checks" (name form `^[0-9A-HJKMNP-TV-Z]{26}\.json$`); S§16 (M§6)
**Files:** `crates/zygo-mail-sync/src/address.rs`, `crates/zygo-mail-sync/tests/address.rs`, `docs/specs/2026-11-22-workspace-mail-design.md` (S§16 change note: M§6 gains the address grammar and the reserved name zygo)
**Consumes:** `valid_mail_host` (MS-3); the member name form and the reserved names cli and all: `valid_member_name` (ML-1), `RESERVED_MEMBER_NAMES` (ML-1)
**Produces:** `zygo_mail_sync::address::Address { pub name: String, pub host: String }`, `zygo_mail_sync::address::parse(s: &str) -> Option<Address>`, `zygo_mail_sync::address::To` (enum: `Local(String)`, `Remote(Address)`, `All`), `zygo_mail_sync::address::AddrError` (enum: `Reserved`, `BadForm`), `zygo_mail_sync::address::resolve_to(s: &str, own_hosts: &[String]) -> Result<To, AddrError>`, `zygo_mail_sync::address::RESERVED: [&str; 3]`, `zygo_mail_sync::address::ulid_file(name: &str) -> Option<ulid::Ulid>`
**Acceptance:** `cargo test -p zygo-mail-sync --test address` → all pass: a bare name is `Local`; `<name>@<own host>` is `Local(<name>)`; `<name>@<other host>` is `Remote`; `all` is `All`; `cli@<host>` and `zygo@<host>` are `Reserved`; a name or host outside its form is `BadForm`; `ulid_file` accepts Crockford ULID names only (no `I`, `L`, `O`, `U`, no lower case, no other suffix).

## Plan
