//! Derived values (K§6.2.4): cell, scores, bar, rollup and release state.

use crate::model::*;

/// The grade of a cell (K-D7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Grade {
    Full,
    Partial,
    None,
    Na,
}

/// A criterion that is not met, or is `n/a`: the keys of a K§6.2.6 JSON `missing` entry.
#[derive(Debug, Clone, PartialEq)]
pub struct Unmet {
    pub criterion: String,
    pub text: String,
    pub reason: Option<String>,
}

/// One cell of the matrix: a client's grade for one capability.
#[derive(Debug, Clone, PartialEq)]
pub struct Cell {
    pub grade: Grade,
    /// met ÷ applicable, for display only; `scores` never reads it.
    pub value: Option<f64>,
    pub missing: Vec<Unmet>,
    pub na: Vec<Unmet>,
}

/// Percent scores, each rounded down once from the exact fraction (K-D8); `None` is `n/a`.
#[derive(Debug, Clone, PartialEq)]
pub struct Scores {
    pub total: Option<u32>,
    pub core: Option<u32>,
    pub standard: Option<u32>,
    pub extra: Option<u32>,
}

/// The core bar (K-D9, K-D18).
#[derive(Debug, Clone, PartialEq)]
pub struct Bar {
    pub meets: bool,
    /// The ids of the core capabilities that fail the bar, in registry order.
    pub failing: Vec<String>,
    /// The criteria of the client's approved bar exceptions, in registry order.
    pub exceptions: Vec<String>,
}

/// The keys of a JSON rollup row.
#[derive(Debug, Clone, PartialEq)]
pub struct Counts {
    pub full: u32,
    pub partial: u32,
    pub none: u32,
    pub na: u32,
    pub met: u32,
    pub applicable: u32,
}

/// A client's rollup: counts per tier and in total, the bar and the release state.
#[derive(Debug, Clone, PartialEq)]
pub struct Rollup {
    pub core: Counts,
    pub standard: Counts,
    pub extra: Counts,
    pub total: Counts,
    pub bar: Bar,
    pub release: &'static str,
}

/// The cell of `capability` for `g`. Panics on an id not in `reg` (a caller bug).
pub fn cell(reg: &Registry, g: &GradeFile, capability: &str) -> Cell {
    let cap = capability_of(reg, capability);
    let (met, applicable) = counts(cap, g);
    let unmet = |c: &Criterion| Unmet { criterion: c.id.clone(), text: c.text.clone(), reason: grade_of(g, &c.id).and_then(|e| e.reason.clone()) };
    let status = |c: &Criterion| grade_of(g, &c.id).map(|e| e.status);
    let grade = match (met, applicable) {
        (_, 0) => Grade::Na,
        (m, a) if m == a => Grade::Full,
        (0, _) => Grade::None,
        _ => Grade::Partial,
    };
    Cell {
        grade,
        value: (applicable > 0).then(|| f64::from(met) / f64::from(applicable)),
        missing: cap.criterion.iter().filter(|c| !matches!(status(c), Some(Status::Met | Status::Na))).map(unmet).collect(),
        na: cap.criterion.iter().filter(|c| status(c) == Some(Status::Na)).map(unmet).collect(),
    }
}

fn capability_of<'a>(reg: &'a Registry, id: &str) -> &'a Capability {
    reg.capability.iter().find(|c| c.id == id).unwrap_or_else(|| panic!("no capability `{id}` in the registry"))
}

/// The grade of a criterion: the first entry for it; `None` when it has no grade (counts as not met).
fn grade_of<'a>(g: &'a GradeFile, criterion: &str) -> Option<&'a GradeEntry> {
    g.grade.iter().find(|e| e.criterion == criterion)
}

/// `(met, applicable)` of a capability: applicable = the criteria that are not `n/a`.
fn counts(cap: &Capability, g: &GradeFile) -> (u32, u32) {
    let status = |c: &Criterion| grade_of(g, &c.id).map(|e| e.status);
    let met = cap.criterion.iter().filter(|c| status(c) == Some(Status::Met)).count() as u32;
    let applicable = cap.criterion.iter().filter(|c| status(c) != Some(Status::Na)).count() as u32;
    (met, applicable)
}

/// The tier and total scores of `g`.
pub fn scores(reg: &Registry, g: &GradeFile) -> Scores {
    let tier = |t: Tier| {
        let values: Vec<Frac> = reg
            .capability
            .iter()
            .filter(|c| c.tier == t)
            .map(|c| counts(c, g))
            .filter(|&(_, applicable)| applicable > 0)
            .map(|(met, applicable)| Frac::new(met.into(), applicable.into()))
            .collect();
        let n = values.len() as u128;
        (n > 0).then(|| values.into_iter().fold(Frac::new(0, 1), Frac::add).div(n))
    };
    let (core, standard, extra) = (tier(Tier::Core), tier(Tier::Standard), tier(Tier::Extra));
    let weighted = [(3, core), (2, standard), (1, extra)];
    let weights: u128 = weighted.iter().filter(|(_, s)| s.is_some()).map(|(w, _)| w).sum();
    let total = (weights > 0).then(|| {
        weighted.iter().filter_map(|&(w, s)| s.map(|s| s.mul(w))).fold(Frac::new(0, 1), Frac::add).div(weights)
    });
    Scores { total: total.map(Frac::percent), core: core.map(Frac::percent), standard: standard.map(Frac::percent), extra: extra.map(Frac::percent) }
}

/// An exact non-negative fraction `num / den` (den > 0), kept reduced, so the rounding is the last step (K§6.2.4).
#[derive(Debug, Clone, Copy)]
struct Frac {
    num: u128,
    den: u128,
}

impl Frac {
    fn new(num: u128, den: u128) -> Frac {
        let d = gcd(num, den);
        Frac { num: num / d, den: den / d }
    }

    fn add(self, o: Frac) -> Frac {
        Frac::new(self.num * o.den + o.num * self.den, self.den * o.den)
    }

    fn mul(self, k: u128) -> Frac {
        Frac::new(self.num * k, self.den)
    }

    fn div(self, k: u128) -> Frac {
        Frac::new(self.num, self.den * k)
    }

    /// ⌊100 × x⌋.
    fn percent(self) -> u32 {
        (100 * self.num / self.den) as u32
    }
}

fn gcd(a: u128, b: u128) -> u128 {
    if b == 0 { a.max(1) } else { gcd(b, a % b) }
}

/// The core bar of `g`.
pub fn bar(reg: &Registry, g: &GradeFile) -> Bar {
    let approved: Vec<&BarException> = reg.bar_exception.iter().filter(|b| b.client == g.client && !blank(&b.approved)).collect();
    let counts = |criterion: &str| match grade_of(g, criterion).map(|e| e.status) {
        Some(Status::Met) => true,
        Some(Status::NotMet) => approved.iter().any(|b| b.criterion == criterion),
        Some(Status::Na) | None => false,
    };
    let failing: Vec<String> = reg
        .capability
        .iter()
        .filter(|cap| cap.tier == Tier::Core && cap.criterion.iter().any(|c| !counts(&c.id)))
        .map(|cap| cap.id.clone())
        .collect();
    Bar { meets: failing.is_empty(), failing, exceptions: approved.iter().map(|b| b.criterion.clone()).collect() }
}

/// The rollup of `g`; `supported` is the client's flag.
pub fn rollup(reg: &Registry, g: &GradeFile, supported: bool) -> Rollup {
    let tier = |t: Tier| {
        let mut n = Counts { full: 0, partial: 0, none: 0, na: 0, met: 0, applicable: 0 };
        for cap in reg.capability.iter().filter(|c| c.tier == t) {
            match cell(reg, g, &cap.id).grade {
                Grade::Full => n.full += 1,
                Grade::Partial => n.partial += 1,
                Grade::None => n.none += 1,
                Grade::Na => n.na += 1,
            }
            let (met, applicable) = counts(cap, g);
            n.met += met;
            n.applicable += applicable;
        }
        n
    };
    let (core, standard, extra) = (tier(Tier::Core), tier(Tier::Standard), tier(Tier::Extra));
    let total = Counts {
        full: core.full + standard.full + extra.full,
        partial: core.partial + standard.partial + extra.partial,
        none: core.none + standard.none + extra.none,
        na: core.na + standard.na + extra.na,
        met: core.met + standard.met + extra.met,
        applicable: core.applicable + standard.applicable + extra.applicable,
    };
    let bar = bar(reg, g);
    let release = match (bar.meets, supported) {
        (false, _) => "below bar",
        (true, true) => "supported",
        (true, false) => "meets bar, not released",
    };
    Rollup { core, standard, extra, total, bar, release }
}
