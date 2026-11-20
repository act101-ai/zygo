/// A 26-character Crockford ULID.
pub fn ulid() -> String {
    ulid::Ulid::generate().to_string()
}
