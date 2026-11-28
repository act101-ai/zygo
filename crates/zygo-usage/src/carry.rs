//! O§6.3: the cost an event carries over the requests of its context that re-send it.

/// USD per Mtok; `write` is `cache_write_5m`, absent: `cache_write`.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Rates {
    pub input: f64,
    pub cache_read: f64,
    pub write: f64,
}

/// One request of a context `Q_C`, as OS-7 reads it from `requests`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CtxRequest {
    pub rowid: i64,
    pub ts: i64,
    pub model: String,
    pub provider: String,
    pub input_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub compaction: bool,
}

/// The sum of the carrying requests' rates in USD per token, and the rowids no rate was found for.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Carry {
    pub usd_per_token: f64,
    pub unpriced: Vec<i64>,
}

fn prompt(r: &CtxRequest) -> u64 {
    r.input_tokens.saturating_add(r.cache_read_tokens).saturating_add(r.cache_write_tokens)
}

/// O§6.3 steps 2-4 for one event: the sum of the carrying requests' rates in USD per token (`T_e` not applied).
/// `q` is `Q_C` in `ts`, `rowid` order; `first_only` is the whole-session rule: the session has a request of a client
/// whose requests carry no per-agent id (`ClientSpec::agent_id_header` is `None`).
pub fn carry(event_ts: i64, q: &[CtxRequest], first_only: bool, rates: &dyn Fn(&str, &str, u64) -> Option<Rates>)
    -> Carry {
    let mut c = Carry::default();
    let Some(start) = q.iter().position(|r| r.ts >= event_ts) else { return c };
    let mut sum = 0.0;
    for i in start..q.len() {
        let (r, prev) = (&q[i], i.checked_sub(1).map(|j| &q[j]));
        if prev.is_some_and(|p| prompt(r).saturating_mul(2) < prompt(p)) { break }
        let rate = rates(&r.model, &r.provider, prompt(r)).map(|x| match () {
            _ if r.cache_read_tokens == 0 && r.cache_write_tokens == 0 => x.input,
            _ if i == start || prev.is_some_and(|p| r.cache_read_tokens < p.cache_read_tokens) => x.write,
            _ => x.cache_read,
        });
        match rate.filter(|v| v.is_finite()) {     // `Prices::cost` drops a non-finite result
            None => c.unpriced.push(r.rowid),
            Some(v) => sum += v,
        }
        if r.compaction || first_only { break }
    }
    c.usd_per_token = sum / 1e6;
    c
}
