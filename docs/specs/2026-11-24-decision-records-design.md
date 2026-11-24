# zygo: delegated decision records — design

**Status:** proposed 2026-11-24
**Date:** 2026-11-24
**Release:** phase 3, track A (roadmap items P3-8, P3-9 and P3-10, `docs/roadmap/phase-3.md`)
**Base specs:** `docs/specs/2026-11-18-zygo-design.md` (`§n`). Also cited: `docs/specs/2026-11-20-workspaces-design.md` (`W§n`, decisions `AUn`), `docs/specs/2026-11-22-workspace-mail-design.md` (`M§n`). `D§n` is a section of this document.

Each decision has a mark. **(operator)**: the operator made the decision (the roadmap's P3-8 and P3-10 design notes, and the plan of work of 2026-11-24). **(author)**: the author made it; the operator can change it before approval. Code references are `file:line` at commit `6e7fe2f`. Skill references are to the act-lite copy of the skills at `plugins/act-lite/skills/` in a zygo session snapshot (§15.3); the act101 plugin carries the same skills.

---

## 1. Intent

The `zygo-<role>` subagents make decisions and assumptions that the operator did not make. A review gate runs 3 rounds at most. Today a finding that is open after round 3 needs the operator, so the work stops.

The operator's direction (2026-11-23): each such decision gets a record, in the same way as an architecture decision record (ADR), but after the fact. The work then passes the 3-round gate and continues with no operator action. The operator audits the records at the end of a phase and makes categorical changes when an assumption was incorrect. No finding stays without a record and a disposition **(operator)**.

This spec settles the record (P3-8), what zygo enforces (P3-9) and the phase-end audit (P3-10).

Today zygo sees none of this. `work_verdict` accepts a `findings` string and drops it: "`findings` is accepted, not stored (§14 stores no free text)" (`crates/zygo-mcp/src/tools/work.rs:58`). It stores only the verdict word on the unit row (`crates/zygo-usage/src/store.rs:288-297`).

## 2. Goals

- **D-G1** Each decision an agent makes under delegation, and each review finding it does not fix, has one durable record in the repository, in the pull request that caused it.
- **D-G2** Each review finding has exactly one disposition: fixed, rejected or recorded.
- **D-G3** A node passes its review gate with no operator action, unless a finding is of a class that stops for the operator.
- **D-G4** One query lists each record that depends on a given assumption, directly or through other records.
- **D-G5** At the end of a phase the operator sees each open record, largest dependency set first, and ratifies, overturns or promotes it.
- **D-G6** zygo refuses a `keep` verdict that breaks the disposition rule, when the operator turns the check on.

## 3. Non-goals

- A numeric limit on open records. Phase 3 measures the count and the dependency depth; the first phase-end review decides if a limit is necessary **(operator)**.
- Work graph edits by zygo. zygo never writes `graph.md` or a node file. An overturn makes a work node through the planning skill (D§12).
- Proof of who typed a ratification. zygo has no user identity (§3). D§11.1 keeps agents out of the operator verbs; it does not sign.
- A check that the lead lists every finding of a review. zygo does not see the reviewer's text (C3, §14). The lead signs the list (D§8.4).
- Records in the usage store, or any record text in a zygo store or log.
- Records for OpenCode subagents through hooks. The record is a file and the verdict is an MCP tool; both work on OpenCode. The lead-only rule of `work_verdict` is not enforced on OpenCode (§8.7 note: no `work_verdict` hook, no agent id), as today.

## 4. Constraints

- **D-C1** C3 and §14 hold. zygo stores no record text, finding text or disposition reason in the usage store, a log or a session file. The record files are repository content that agents write, as code is.
- **D-C2** C6 holds. No hook reads a record file (D§14). The verdict check is in the MCP tool, which can refuse; it is not a hook.
- **D-C3** The record file is the one owner of a record's state. zygo stores no copy and no index. Each derived value (dependents, depth, counts) is computed from the files at each read.
- **D-C4** A record is never deleted or renamed. A record leaves the open set only by a status change (D§6.6).
- **D-C5** An agent never writes an operator field or an operator status (D§6.6). Only the operator verbs (D§11) write them.
- **D-C6** Every new input is bounded: tool arguments (D§9.2), record files (D§6.8), the number of files read (D§6.8), and each git call (2 s, as the §13.5 note).
- **D-C7** The work-unit rules of §13 hold. The new guard (D§9.3) follows the §13.4 levels and the §13.5 resolution.
- **D-C8** The skills are outside this repository. This spec names the skill changes (D§12); the plugin makes them **(operator)**.

## 5. Terms

| Term | Meaning |
|---|---|
| DDR | Delegated decision record. The agent decided under delegated authority and the operator has not ratified the decision. "PDR" is not used: it is a known term (preliminary design review) **(operator)**. |
| Record | One DDR file under `docs/decisions/` (D§6). |
| Finding | One defect that a reviewer reports in one review round of one node, with its location, the failure and the evidence (execute-work `SKILL.md:146-158`). |
| Disposition | The end state of one finding: `fixed`, `rejected` or `recorded` (D§8.1). |
| Stop class | A class of finding or record that an agent cannot record and continue past (D§8.2). |
| Dependent | A record B is a dependent of record A when A is in B's `depends_on`, or in the `depends_on` of a dependent of A. |
| Signer | The session that submits the dispositions: the lead, through `work_verdict` (D§8.4). |
| Operator verb | `zygo decision ratify`, `overturn`, `promote` or `review` (D§11). |

## 6. The record (P3-8)

### 6.1 Location and id

**(operator)** One file per record in the repository, under `docs/decisions/`, in the pull request that caused it.

**(author)** The repository is the one that holds the work: the repository root of the checkout where the record is written (`git rev-parse --show-toplevel`). The id is `DDR-` and a ULID in upper-case Crockford base32: `^DDR-[0-9A-HJKMNP-TV-Z]{26}$`. The file is `docs/decisions/<id>.md`. The file name and the `id` key are equal.

Reason for a ULID: parallel nodes write records in separate checkouts (execute-work `SKILL.md:202-207`), and phase 3 adds machines. A sequence number (`DDR-0042`) collides between two checkouts and needs a counter that someone owns. A ULID needs no coordination, and the file names sort by time. M§5 uses ULIDs for the same reason.

### 6.2 File format

**(author)** Markdown with a TOML front matter between two lines that hold only `+++`. TOML because zygo already parses and edits TOML (`toml`, `toml_edit` in `Cargo.toml:66-67`); YAML would add a parser. An edit by zygo uses `toml_edit`, so it changes only the keys it sets and keeps unknown keys, comments and the body byte-equal (as AU78 keeps unknown fields).

```markdown
+++
id = "DDR-01JDQ2Y7Z8K4M5N6P7Q8R9S0TV"
kind = "decision"
status = "open"
class = "none"
title = "Store the pane name in the member record, not in the session view"
unit = "work-graph:phase-3-N12"
round = 2
role = "code"
model = "claude-sonnet-4-5"
commit = "6e7fe2f0c1d2e3f4a5b6c7d8e9f00112233445566"
date = "2026-11-24"
depends_on = ["DDR-01JDQ2W0000000000000000000"]
reverse_cost = "low"
+++

## Choice

One paragraph: what the agent chose.

## Alternatives rejected

Each alternative, and why it was rejected.

## Evidence

The file:line, test output or document text that the choice rests on.

## Cost to reverse

What a reversal changes, and which later work it touches.
```

### 6.3 Fields

**(operator)** The fields come from the P3-8 notes: id; work unit; role and model; commit; date; the choice; the alternatives rejected; the evidence; the assumptions it depends on, by id; the cost to reverse; the later records that build on it; status. **(author)** The table fixes each one.

| Key / section | Type and limit | Written by | Meaning |
|---|---|---|---|
| `id` | D§6.1 form | agent, at creation | Equals the file stem. |
| `kind` | `assumption`, `decision`, `accepted-limit`, `compromise`, `open-finding` | agent, at creation | D§6.4. |
| `status` | `open`, `ratified`, `overturned`, `fixed`, `superseded` | agent or operator, by D§6.6 | D§6.5. |
| `class` | `none` or a stop class of D§8.2 | agent, at creation | The stop class of the subject, else `none`. |
| `title` | one line, 1 to 120 bytes | agent | Short name for lists. |
| `unit` | §13.1 unit id | agent, at creation | The work unit that made the decision. |
| `round` | integer ≥ 1 | agent, at creation | The unit's round (§13.1). |
| `role` | a role name of the session (`code`, `lead`, …) | agent, at creation | The role that made the decision. `lead` when the decision is in the lead's brief. |
| `model` | catalog id, or `unknown` | `zygo decision new` (D§10.2) | The model that served the role. |
| `commit` | 40 or 64 hex | `zygo decision new` | `HEAD` of the checkout when the record was written: the code the decision was made against. |
| `date` | `YYYY-MM-DD` | `zygo decision new` | Creation date, UTC. |
| `depends_on` | array of ids, at most 64 | agent | The records this one builds on: the assumptions it depends on, and any other record. |
| `reverse_cost` | `low`, `medium`, `high` | agent | `low`: one node or less. `medium`: more than one node, and no public contract or stored format. `high`: a public contract, a stored format, a migration or data. |
| `supersedes` | one id, optional | agent | The record that this one replaces (D§6.6). |
| `fixed_by` | 40 or 64 hex | agent | The commit that fixed the subject. Required when `status = "fixed"`. |
| `promoted_by` | 40 or 64 hex | agent | The commit that wrote the promotion (D§11.3). |
| `reviewed` | `YYYY-MM-DD` | operator verb | Date of the operator's outcome. |
| `review_note` | text, at most 2,000 bytes | operator verb | The operator's reason. Required for `overturned`. |
| `promoted_to` | `adr`, `spec`, `rule` | operator verb | D§11.3. |
| `promotion` | text, at most 500 bytes | operator verb | Where the promotion goes and what it says. Required with `promoted_to`. |
| `## Finding` | Markdown section | agent | Required for a record made from a finding (D§8.1): the reviewer's finding, with its location, failure and evidence. |
| `## Choice` | Markdown section, not empty | agent | The choice. For `open-finding`: what stays open. |
| `## Alternatives rejected` | Markdown section, not empty | agent | For `open-finding`: the fixes considered, and why none was made. |
| `## Evidence` | Markdown section, not empty | agent | |
| `## Cost to reverse` | Markdown section, not empty | agent | The detail of `reverse_cost`. |

**(author)** "The later records that build on it" is not stored. It is the inverse of `depends_on`, computed at each read (D-C3). Reason: a stored list is a second owner of one fact. Two parallel nodes that each add a dependent would both edit the earlier record, and their merges would conflict; a list edited by hand drifts from the `depends_on` keys. `zygo decision show` and `deps` print it (D§10).

Unknown keys are kept and ignored. Unknown sections are kept.

### 6.4 Kinds

**(operator)** One record type, with a kind. **(author)** The meaning of each kind:

| Kind | Use |
|---|---|
| `assumption` | A fact the agent took as true without proof (an API behavior, a premise about the user). Other records cite it. |
| `decision` | A choice between alternatives that the spec or the brief did not make. |
| `accepted-limit` | A confirmed finding that stays as it is, because the limit is acceptable. |
| `compromise` | A confirmed finding fixed in part, or fixed by a means other than the one the spec states. |
| `open-finding` | A confirmed finding that is not fixed and not accepted: a defect left for the operator. |

### 6.5 Statuses

**(operator)** `open`, `ratified`, `overturned`, `fixed`, `superseded`.

| Status | Meaning |
|---|---|
| `open` | Written by an agent. The operator has not seen it. |
| `ratified` | The operator accepted it. With `promoted_to`: accepted and promoted (D§11.3). |
| `overturned` | The operator rejected it. A work node reverses it (D§11.3). |
| `fixed` | A commit fixed its subject (`fixed_by`). |
| `superseded` | A later record replaced it (that record's `supersedes`). |

### 6.6 Transitions

**(author)** Only these transitions are allowed. Each one is made in a commit that carries the trailer of D§6.7.

| From | To | By | Writes |
|---|---|---|---|
| none | `open` | agent | A new file. Operator keys absent; `fixed_by` and `promoted_by` absent. |
| `open` | `open` | agent | Agent keys and sections. Operator keys stay absent. |
| `open` | `fixed` | agent | `status`, `fixed_by`. |
| `open` | `superseded` | agent | `status`. A record in the same node's head names it in `supersedes`. |
| `overturned` | `fixed` | agent | `status`, `fixed_by` (the commit that carried out the reversal). |
| `ratified` with `promoted_to` and no `promoted_by` | the same | agent | `promoted_by` only. |
| `open` | `ratified` | operator verb | `status`, `reviewed`, `review_note` (may be empty); with promote also `promoted_to`, `promotion`. |
| `open` | `overturned` | operator verb | `status`, `reviewed`, `review_note` (not empty). |
| `ratified` | `overturned` | operator verb | As above. The operator can change an outcome. |

`fixed` and `superseded` are final. An agent never changes a `ratified` record except for `promoted_by`, and never makes a record `ratified` or `overturned` (D-C5). The failure this prevents: an agent that ratifies its own record removes it from the operator's audit, and the audit is the purpose of the track.

### 6.7 The `Zygo-Decision:` trailer

**(operator)** P3-9 adds a commit trailer. **(author)** Form: `Zygo-Decision: <id>`, one line per id, at the end of the message with the other trailers (§13.4). A commit carries it when it:

1. adds or changes a record;
2. carries out a decision (the code that a record of kind `decision` or `compromise` describes);
3. fixes the subject of a record (`fixed_by`), or writes a promotion (`promoted_by`).

Use: `git log --grep '^Zygo-Decision: <id>$'` lists each commit made under a record. An overturn needs this list: it is the set of changes to reverse. The commit that adds a record file is also found by `git log --diff-filter=A -- <file>`; the trailer is for item 2, which does not touch the record file.

No guard checks the trailer (D§9.6).

### 6.8 Limits

**(author)** A record file is valid when all of these hold; else it is malformed:

- a regular file (`lstat`; a link, a FIFO or a directory is never opened), at most 64 KiB, UTF-8;
- line 1 is `+++`, a second `+++` line follows within the first 8 KiB, and the text between them parses as TOML;
- each required key is present with its D§6.3 type and limit; `id` equals the file stem;
- the four required sections exist and are not empty, and `## Finding` exists for `accepted-limit`, `compromise` and `open-finding`. A section that still holds the marker `<!-- zygo: fill -->` (D§10.2) counts as empty;
- the status keys agree: `fixed` has `fixed_by`; `overturned` has a non-empty `review_note`; `promoted_to` has `promotion`.

A reader considers at most 10,000 entries of `docs/decisions/`. Above that, a listing verb exits 1 with `zygo decision: more than 10,000 files in docs/decisions/`, and the report prints the D§9.5 line with `over 10,000 files`. Files whose names do not end in `.md`, and `README.md`, are ignored.

Set checks (D§10.1 `check`): ids are unique; each `depends_on` and `supersedes` id names a record; `depends_on` has no cycle; the target of `supersedes` has status `superseded`.

## 7. Assumptions and the dependency query

**(operator)** Assumptions are first-class. A decision cites an assumption by id. When an assumption is found incorrect, one query lists each record that depends on it.

**(author)** An assumption is a record of kind `assumption`. A record cites it in `depends_on`. `depends_on` can also name a record of another kind, because a decision can build on a decision.

The query is `zygo decision deps <id>` (D§10.1). It prints each dependent of `<id>`, breadth first, one per line: depth (1 for a direct dependent), id, kind, status, title. A record reached on two paths is printed once, at its smallest depth. A cycle (only a hand edit makes one) is cut at the first repeat, and the verb prints `zygo decision: cycle at <id>` on stderr.

Malformed files are not in the graph; the verb prints one stderr line per malformed file and exits 0. A malformed file can hide a dependent: `zygo decision check` (exit 1) is the guard against that, and the deliver step runs it (D§12).

## 8. The disposition rule (P3-8)

### 8.1 Dispositions

**(operator)** Each review finding ends in exactly one disposition:

| Disposition | Proof |
|---|---|
| `fixed` | a commit |
| `rejected` | a reason |
| `recorded` | a DDR id |

**(author)** The act-lite verification stage marks each finding confirmed or refuted (execute-work `SKILL.md:159-161`). A refuted finding is `rejected`, with the evidence as the reason. A confirmed finding is `fixed`, or `recorded` with a record of kind `accepted-limit`, `compromise` or `open-finding`.

**(operator, plan of work 2026-11-24, decision 3)** Each work node has its own review gate of up to 3 rounds. After round 3, a node passes only when each finding has exactly one disposition, and no finding is of a stop class.

**(author)** `recorded` is accepted only in round 3 or later. In rounds 1 and 2 a confirmed finding is fixed, or the round fails and a new round starts. Reason: the operator's direction is that a finding open *after round 3* gets a record. Without this limit an agent can record each finding in round 1, skip the review loop, and move the work to the operator's audit. A finding that is not a defect is `rejected` with its reason in any round.

### 8.2 Stop classes

**(operator)** An agent cannot record these and continue:

| Class value | Class |
|---|---|
| `acceptance` | a failed acceptance check |
| `security` | a security finding |
| `irreversible` | data loss, or an action that cannot be reversed |
| `contract` | a change to a public contract: CLI, config schema, MCP tool shape, stored format |
| `spec` | a deviation from a constraint in an approved spec |
| `rule` | a new rule, or a permission change |

**(author)** The rule applies to findings and to records:

- A finding of a stop class passes only as `fixed` or `rejected`. `recorded` stops the node.
- A new record with `class` other than `none` stops the node, whatever its kind. An agent that decides alone to change a CLI flag cannot record that and continue.
- A stopped node waits for the operator. The skill sets it `BLOCKED(stop: <class>)` and asks the operator (D§12).

**(author)** A stop-class finding can be `rejected` with a reason. A refuted security finding is not a security defect, and a stop on each refuted finding would stop each node that a strict reviewer reads. The deliver step lists each rejected stop-class finding in the pull request body with its reason (D§12), so the operator sees each one.

### 8.3 What an author decides alone

**(author)** Without a record: mechanics (structure, order, internal names, how to test, how to fix a finding: execute-work `SKILL.md:55-56`). With a record of class `none`: a choice the spec and the brief leave open, an assumption, an accepted limit, a compromise, an open finding. Never: a stop class (D§8.2). The plan step writes this into the graph header (D§12).

### 8.4 Who signs

**(operator)** The author does not accept its own compromise. The reviewer or the lead signs the disposition.

**(author)** The signature is the `work_verdict` call. Only the lead can make it on Claude Code: the `PreToolUse` hook denies it in a subagent (`crates/zygo-work/src/hooks.rs:183-186`, text at `:164`; hook registered at `crates/zygo-launch/src/claude_code.rs:484`). The implementer and the reviewer are subagents, so neither can sign.

The act-lite skills call the lead "the author" when a subagent writes text from the lead's brief (execute-work `SKILL.md:49-54`). Thus a decision that is in the lead's brief (`role = "lead"`) is the lead's own. For such a record the reviewer signs: the reviewer's report states, for each finding the lead plans to record, that it accepts the record, and the lead submits it. zygo cannot see the reviewer's text (D-C1), so this part is a skill duty (D§12).

## 9. Enforcement in zygo (P3-9)

### 9.1 Data path

**(author)** zygo learns the findings and their dispositions from the `work_verdict` call itself, and the records from the git commit the call names. No new store.

1. The reviewer reports findings to the lead (skill).
2. The implementer writes each record with `zygo decision new` in the node's checkout and commits it with the trailer (D§6.7).
3. The lead calls `work_verdict` with `verdict = "keep"`, the node branch's head commit as `head`, and one `dispositions` entry per finding. The lead calls it **before** the merge into the work branch (D§12): the call is the gate.
4. The tool checks the entries against the commit `head` (D§9.4). Git objects are shared by all linked checkouts, so the lead's repository reads the node's commits.
5. The durable copy of the disposition list is the node file's `## Review` section and the pull request body (skill, D§12). zygo keeps no text (D-C1).

A file written by the reviewer was rejected: `zygo-review` has no edit tool (`writes = false`, `crates/zygo-config/data/roles.toml:27-31`, so `disallowedTools`, §8.5), and the lead cannot edit (§8.3). A trailer was rejected: a disposition list with reasons does not fit trailers, and a finding fixed in round 1 has no commit of its own.

### 9.2 `work_verdict` input

**(author)** Two optional keys are added to the input schema (`crates/zygo-mcp/src/tools/work.rs:50-55`). `findings` stays a string and keeps its meaning. `verdict` keeps its three values (`work.rs:16`).

```json
"head": {"type": "string", "pattern": "^[0-9a-f]{7,64}$"},
"dispositions": {"type": "array", "maxItems": 256, "items": {"type": "object", "properties": {
  "finding":     {"type": "string", "minLength": 1, "maxLength": 200},
  "class":       {"type": "string", "enum": ["none","acceptance","security","irreversible","contract","spec","rule"]},
  "disposition": {"type": "string", "enum": ["fixed","rejected","recorded"]},
  "commit":      {"type": "string", "pattern": "^[0-9a-f]{7,64}$"},
  "reason":      {"type": "string", "minLength": 1, "maxLength": 2000},
  "record":      {"type": "string", "pattern": "^DDR-[0-9A-HJKMNP-TV-Z]{26}$"}
}, "required": ["finding","class","disposition"]}}
```

`finding` is the lead's short label for the finding (for example `R2-3 unbounded read in report`). The description gains one sentence: "On keep, give `head` (the commit you judged) and one entry in `dispositions` for each review finding: fixed with its commit, rejected with its reason, or recorded with its DDR id."

An entry with too many keys for its disposition (for example `commit` on `rejected`) is an error. The whole argument object is at most 256 KiB; a larger one is refused before any check, with `InvalidParams`.

### 9.3 The `dispositions` guard

**(author)** The check is a guard of §13.4, so its level comes from config, `.zygo.toml` and the session override, with the `min_level` floor (§13.5). Reason: a repository and a user can then turn it on with the existing surfaces (`[guards]`, `/zygo:guards`), and a user who does not use records is not refused.

| Guard | Trigger | Event when | notify text / enforce action |
|---|---|---|---|
| dispositions | `work_verdict` with `verdict = "keep"` | any problem of D§9.4 | the verdict is recorded and the reply ends with `zygo: dispositions: <problems>` / the verdict is not recorded; the tool returns an error with `zygo: keep refused: <problems>` |

- Default level: `monitor`, as the other guards (§13.4). This guard has no place to record an event (next item), so at `monitor` the check does not run and the tool behaves as today.
- The guard runs in the MCP tool, not in a hook. It records no `guard` line and no count in `guard_events`: that column is written by the fold, which replaces it on each fold (`crates/zygo-usage/src/store.rs:252`), so the tool cannot own a part of it. The reply is the only output.
- Phase 3: the operator sets `dispositions = "enforce"` in user config `[guards]` once a binary with this item is installed. A `.zygo.toml` on the default branch does not reach the `phase-3` branch until it merges (§13.5). Until then the skill applies the rule (plan of work, decision 3).
- `<problems>` is at most 10 problems joined by `; `, then `; and <n> more`, at most 4 KiB.

### 9.4 The checks

**(author)** The tool runs the checks in this order and collects each problem. Git runs in the session's repository root (D§13 item 6), each call bounded to 2 s, all calls of one verdict bounded to 10 s.

1. `head` is present and names a commit (`git rev-parse --verify <head>^{commit}`). Else: `no head commit`.
2. `dispositions` is present. An empty array is valid (no findings). Else: `no dispositions`.
3. Each `finding` label is unique. Else: `finding <label> has two dispositions`.
4. `fixed`: `commit` is present, names a commit, and is `head` or an ancestor of it (`git merge-base --is-ancestor`). Else: `<label>: commit <c> is not in <head>`.
5. `rejected`: `reason` is present.
6. `recorded`: the verdict's `round` is 3 or more (D§8.1); `record` is present; `docs/decisions/<record>.md` exists in `head` (`git cat-file -s` first: at most 64 KiB; then `git show`) and is valid (D§6.8), with status `open` and kind `accepted-limit`, `compromise` or `open-finding`; and each id in its `depends_on` exists in `head`. Else: `<label>: <why>`.
7. Stop class: a `recorded` entry whose `class` is not `none`. Problem: `<label>: class <class> stops for the operator`.
8. Record changes: base = `git merge-base HEAD <head>`. For each path under `docs/decisions/` that `git diff --name-status <base> <head>` lists (at most 1,000; above that: `too many record changes`): a deleted or renamed record is a problem; else the old and new files are read from `base` and `head`, and each change must be an agent transition of D§6.6. A new record with `class` other than `none` is a problem (D§8.2). A changed file that is malformed in `head` is a problem. When `base` equals `head` (the lead already merged the node, or works in one checkout), there is nothing to compare: check 8 is skipped and the reply, at `notify` and `enforce`, ends with `zygo: record changes not checked: <head> is already in HEAD; call work_verdict before the merge`. This keeps a one-checkout workflow usable at `enforce`; the skill order of D§12 item 10 makes the check run.

No other files are read. A malformed record that the node did not touch is not the node's problem; `zygo decision check` reports it.

A git call that fails or passes its bound gives the problem `cannot check: <git step> <why>`. At `enforce` the keep is refused, and the lead can call again. Reason for refusing: this is the gate, and a check that passes on error does not gate. It is not a hook, so C6 does not require the call to pass (D-C2).

### 9.5 `zygo work report`

**(operator)** `zygo work report` counts open records. **(author)** The output of `crates/zygo-work/src/report.rs` gains two lines. Records are read from the working tree of the repository at the current directory (`git rev-parse --show-toplevel`, 2 s bound).

- In each unit block, after the `guard events:` line (`report.rs:82`), when at least one valid record has this `unit`, or this unit with a `:<part>`: `  decisions: <open> open, <total> total`.
- After the last block, after one empty line, when `docs/decisions/` holds at least one `.md` file: `decision records: <open> open, <total> total, deepest chain <d>`, then `, <m> malformed` when `m > 0`. `<d>` is the number of records on the longest `depends_on` path among valid records (a record with no `depends_on` counts 1; a cycle is cut at the first repeat). With more than 10,000 files: `decision records: over 10,000 files`.
- With no units and at least one record, the report prints `no work units` and then the records line.
- Outside a repository, or when the git call fails: no records lines. The report never fails because of a record.

### 9.6 No guard on the trailer

**(author)** No guard checks `Zygo-Decision:`. The trailers guard reads the message text only (`crates/zygo-work/src/guards.rs:111-115`, `:1149-1152`). To check that a commit that adds a record carries the trailer, the hook would run `git diff --cached` on each `git commit`, and the forms `git commit -a` and `git commit <paths>` change the file set. The failure it would prevent is small: the record's commit is found by its path (D§6.7). The trailer that matters (item 2 of D§6.7) touches no record file, so no hook can tell that it is missing.

## 10. CLI

### 10.1 `zygo decision`

**(author)** One new verb. Each subcommand acts on the repository at the current directory.

| Command | Does |
|---|---|
| `zygo decision new --kind <k> --title <t> --unit <u> --round <n> --role <r> [--class <c>] [--depends-on <id>]… [--supersedes <id>]` | Writes a new record (D§10.2), prints its path. |
| `zygo decision ls [--status <s>] [--kind <k>] [--unit <prefix>] [--pending]` | One line per valid record: id, kind, status, class, dependents count, title. `--pending`: `overturned` records, and `ratified` records with `promoted_to` and no `promoted_by`. Default order: id. |
| `zygo decision show <id>` | The record as written, then its direct and total dependents, then the commits whose message has `Zygo-Decision: <id>` (at most 50, newest first). |
| `zygo decision deps <id>` | D§7. |
| `zygo decision check` | All D§6.8 checks. One line per problem; exit 1 when any. |
| `zygo decision ratify <id> [--note <text>] [--promote adr\|spec\|rule --as <text>] [--yes]` | Operator verb (D§11.3). |
| `zygo decision overturn <id> --reason <text> [--yes]` | Operator verb (D§11.3). |
| `zygo decision review [--kind <k>] [--unit <prefix>]` | The phase-end review (D§11). |

`new`, `ls`, `show`, `deps` and `check` exit 2 outside a git repository. `show` and `deps` exit 1 when `<id>` is not a valid record. Promote is `ratify --promote`, because a promotion is a ratification with a target (D§6.5); `--promote` without `--as` exits 2. `ratify` of a record that is not `open`, and `overturn` of a record that is not `open` or `ratified`, exit 1 with `zygo decision: <id> is <status>`, and change nothing.

Text from a record (title, body, notes) and from git (commit subjects) is printed one line at a time through `zygo_core::esc` (`crates/zygo-core/src/text.rs:4`), so an agent-written record cannot send terminal escapes to the operator's terminal. `review` uses the same rule.

Slash form (§17.1): `/zygo:decision [ls|show|deps|check] …` runs the read-only subcommands. Each other subcommand replies `run in your shell: zygo decision <args>` and changes nothing, as the code approval verbs do (§17.1).

### 10.2 `zygo decision new`

**(author)** Fills `id` (a new ULID), `status = "open"`, `date`, `commit` (`git rev-parse HEAD`), and `model`: inside a session, the `model` of the newest `requests` row of this session whose `role_served` is `<r>` (§14); else `unknown`. In serve mode the rows reach the local store only at exit (§14), so it writes `unknown`. The arguments are validated: `--unit` by §13.1, `--round` ≥ 1, `--role` a role of the session's legend or `lead` (outside a session: `^[a-z][a-z0-9-]{0,31}$`), each id by D§6.1 and present as a valid record.

It writes the front matter and the four (or five, D§6.3) section headings, each with a one-line hint for the agent to replace, in `docs/decisions/<id>.tmp`, then renames it to `<id>.md`. It creates `docs/decisions/` when absent. It does not commit. Each hint line is `<!-- zygo: fill --> <hint>`. A write failure leaves at most the `.tmp` file, which every reader ignores; the verb exits 1.

Reason for the verb: a record written by hand gets a bad ULID, a wrong key name or a missing section, and the error shows only at the verdict. The hint lines make an unedited record malformed (`check` fails on them), so a template that the agent forgot to fill cannot pass.

## 11. Phase-end ratification (P3-10)

### 11.1 Who can run it

**(author)** The operator verbs (`ratify`, `overturn`, `review`) are refused, exit 2, when `ZYGO_SESSION_DIR` is set, with `zygo decision <verb>: run it in your own shell, not in an agent session`. This is the rule for code approval (§17.1, N30 note). `review` also needs a terminal on stdin and stdout; else exit 2 with `zygo decision review: needs a terminal; use ratify or overturn with --yes in scripts`. `ratify` and `overturn` show the record and ask `[y/N]`, unless `--yes`.

Known limit: an agent can unset the variable in its shell. D§9.4 check 8 refuses a node whose commits change an operator field, so an agent ratification does not pass a gate at `enforce`.

### 11.2 The review

**(operator)** The operator gets each open record to ratify, overturn or promote. Records are shown sorted by the number of records that build on them, largest first.

**(author)** `zygo decision review`:

1. Read all records (D§6.8). Print one stderr line per malformed file: it cannot be reviewed; `check` names the fault.
2. Take the valid `open` records (filtered by `--kind`, `--unit`). Count each one's dependents over all valid records that are not `superseded`, transitively (D§5). Sort by that count, largest first; then by id (oldest first).
3. For each record, print: `[i/N] <id> <kind> class <class> dependents <n>`; the title; unit, round, role, model, date; each `depends_on` id with its status, with `(overturned)` marked; the body; the commits with its trailer (at most 20). Then ask: `r ratify · p promote · o overturn · s skip · q quit`.
4. Write the outcome (D§11.3) at once, before the next record.
5. After an overturn, the record's open dependents move to the front of the queue, in the step 2 order, each with the line `depends on overturned <id>`. Reason: the operator decides the records that built on a wrong assumption while that assumption is in mind. This is the categorical change of the intent.
6. At the end, or at `q`: print the counts per outcome, the changed files, and `commit these files; add a Zygo-Decision: line per id`. The verb does not commit: the operator's checkout and branch are the operator's.

A crash or `q` loses nothing: each outcome is in its file, and the next `review` shows only the records still `open`.

### 11.3 Outcomes

| Outcome | Asks | Writes (D§6.6) | What follows |
|---|---|---|---|
| Ratify | an optional note | `status = "ratified"`, `reviewed`, `review_note` | Nothing. |
| Promote **(operator)** to an ADR, a spec change or a rule | the target (`adr`, `spec`, `rule`) and the promotion text (not empty) | as ratify, and `promoted_to`, `promotion` | The planning skill makes a node that writes the ADR, the spec text or the rule, and sets `promoted_by` (D§12). |
| Overturn **(operator)**: this makes a work node | the reason (not empty) | `status = "overturned"`, `reviewed`, `review_note` | The planning skill makes a node that reverses the commits of D§6.7 and sets `status = "fixed"`, `fixed_by` (D§12). |
| Skip | — | nothing | The record stays `open`. |

**(operator)** A record never becomes a rule without this review. Only `ratify --promote rule` writes `promoted_to = "rule"`, and D§6.6 keeps agents from writing it. The node that writes the rule cites the record and its `reviewed` date, which the rule clause of the skills accepts as the operator's approval (execute-work `SKILL.md:8-15`).

**(author)** Before each write the verb reads the file again. When its bytes differ from what the verb showed, it writes nothing and prints `<id> changed since it was shown; skipped`. This covers two reviews at once and an edit during a review. The write is a temporary file in `docs/decisions/` and a rename, so a crash leaves the old file or the new one.

## 12. Changes outside this repository

**(operator)** The plan, execute and deliver skills are in the act101 plugin; the act-lite plugin carries the same skills. P3-8 names the changes; the plugin makes them. **(author)** The changes, each against the act-lite text:

**plan-work** (`plan-work/SKILL.md:126-176`, header in `work-graph/references/graph-format.md:36-57`):

1. The graph header gets a `**Delegation:**` line: what an author decides alone, what it records, and the stop classes (D§8.3, D§8.2), citing this spec.
2. When `zygo decision ls --pending` lists records, the plan has one node per record: reverse an `overturned` record (its commits, D§6.7) and set it `fixed`; or write a promotion and set `promoted_by`. Each commit carries `Zygo-Decision: <id>`.

**execute-work** (`execute-work/SKILL.md:140-164` verification stage, `:215-230` keep or discard):

3. The reviewer reports, for each finding, a label and a class (D§8.2), with the location, failure and evidence.
4. Rounds 1 and 2: a confirmed finding is fixed, or the round fails. Round 3: a confirmed finding that is not fixed is recorded (`zygo decision new`, kind `accepted-limit`, `compromise` or `open-finding`, `## Finding` filled from the report). A refuted finding is rejected with its evidence.
5. At the verification stage the implementer also records each assumption and decision of D§8.3 that the node made.
6. For a record with `role = "lead"`, the reviewer of that round states that it accepts the record (D§8.4).
7. The stage passes when each finding has one disposition and nothing has a stop class (D§8.2). A stop: set the node `BLOCKED(stop: <class>)` and ask the operator.
8. Each record commit carries `Zygo-Decision: <id>`.
9. The node file gets a `## Review` section per round: each finding label, class, disposition and its proof.
10. Call `work_verdict` with `keep`, `head` and `dispositions` **before** the merge of §5.5 step 1. A refused keep is a failed stage. Today the skill calls it after the keep or discard (`SKILL.md:224-230`).
11. Two current defects in the same step, found in this design's recon. The skill sends `verdict = "blocked"`, which zygo refuses (`crates/zygo-mcp/src/tools/work.rs:16`, `:68-72`): send `discard` for a `BLOCKED` node. The skill sends `findings` as a number of findings, and zygo refuses a value that is not a string (`work.rs:73-75`): send it as a string.

**deliver-work** (`deliver-work/SKILL.md:94-97`, pull request body):

12. Run `zygo decision check` with the step 2 checks.
13. The body lists each record added or changed on the branch (`git diff --name-status <base>...HEAD -- docs/decisions/`): id, kind, status, class, title. It lists each rejected finding of a stop class with its reason (D§8.2). It gives the `decision records:` line of `zygo work report` (D§9.5).

## 13. Base-spec changes

**(author)** When this spec is approved, the base spec changes as below. Each change is written into the section as final text, with a change-log line.

1. §13.3 `work_verdict`: the `head` and `dispositions` input (D§9.2) and the check (D§9.4).
2. §13.4: one row for the `dispositions` guard (D§9.3). The sentence "All guards except `lead`, `redaction` and `attribution` run only when `[features] work_units` is on" holds for it.
3. §13.5: `dispositions` is a valid key of `[guards]`.
4. §13.6: the two report lines (D§9.5).
5. §17 and §17.1: the `zygo decision` verb and its slash form (D§10.1).
6. The session view (`crates/zygo-core/src/session_view.rs:7-42`) gains `repo_root: Option<String>` with `#[serde(default)]`: the repository top level that the launcher already holds (`crates/zygo-launch/src/launcher.rs:633`). The `work_verdict` tool runs git there. A view without it (an older launcher) makes check 1 fail with `cannot check: no repository root`.

§14 does not change: no column, no table.

## 14. Failure handling

**(author)** No hook reads a record (D-C2). The entry points that read records, and their behavior:

| Case | `work_verdict` | `zygo work report` | `ls`, `show`, `deps` | `check` | `review`, `ratify`, `overturn` |
|---|---|---|---|---|---|
| `docs/decisions/` absent | checks 6 and 8 find no record: problems only for a named `record` | no records lines | empty list, exit 0 | exit 0 | `no open records`, exit 0 |
| A malformed file | a problem only when the node names it or changes it | counted in `malformed` | one stderr line per file, skipped | one line per fault, exit 1 | one stderr line; it cannot be reviewed or changed |
| A file over 64 KiB, a link, a FIFO | as malformed, never opened | as malformed | as malformed | as malformed | as malformed |
| A git call fails or passes 2 s | `cannot check: …` (refused at `enforce`) | no records lines | the git-backed part (commits in `show`) is left out with one stderr line | as for `ls` | as for `show` |
| The write of an outcome fails | — | — | — | — | exit 1 naming the file; earlier outcomes are kept |
| Two writers of one record | — | — | — | — | D§11.3 re-read: skipped |

Two parallel nodes: each writes new files with new ULIDs, so their merges do not conflict. Two nodes that change the same record conflict at the merge of §5.5 step 1; the skill already handles a failed merge as a new round (execute-work `SKILL.md:319-322`).

## 15. Owners of state

| State | Owner | Readers |
|---|---|---|
| A record's fields, body and status | its file in git | all `zygo decision` subcommands, `work_verdict`, `zygo work report` |
| Dependents, counts, depth | none: computed at each read | as above |
| Dispositions | the `work_verdict` call (the lead's signature) | the tool, once. Durable copies: node file `## Review`, pull request body (skills) |
| The verdict word | `work_units.verdict` (§13.3), unchanged | `zygo work report` |
| Commits made under a record | git history, by trailer | `show`, `review`, the planning skill |
| The guard level | §13.5 resolution | the tool |

## 16. Alternatives rejected

- **A sequence number id** (`DDR-0042`): collides between parallel checkouts and between machines; needs a counter with an owner (D§6.1).
- **YAML front matter**: a new parser dependency for no gain over TOML.
- **A stored "built on by" list in each record**: a second owner of the `depends_on` fact, and merge conflicts between parallel nodes (D§6.3).
- **Records in the usage store**: breaks §14 (no free text), and the store is per machine while the records must travel with the pull request.
- **A findings file written by the reviewer**: the reviewer cannot write, and the lead cannot edit (D§9.1).
- **The check in the `PreToolUse` hook of `work_verdict`**: C6 makes a hook pass on any internal error, so a gate there opens on error. The tool can refuse.
- **The gate always on**: refuses each keep of a user who does not use records. The guard level makes it opt-in (D§9.3).
- **A count of disposition events in `guard_events`**: the fold rewrites that column (D§9.3).
- **A guard on `Zygo-Decision:`**: D§9.6.
- **zygo writes the work node of an overturn**: the controller is the only writer of `graph.md` (execute-work `SKILL.md:192-193`), and other work sources have no node format.
- **zygo commits the outcomes of a review**: zygo would change the operator's branch and index; the operator commits.
- **The `--since <ref>` filter on `ls` for the pull request body**: `git diff --name-status` gives the same list (D§12 item 13).
- **A bulk "overturn all dependents"**: not asked. D§11.2 step 5 shows the dependents next, one by one.
- **`recorded` in any round**: lets an agent skip the review loop (D§8.1).

## 17. Acceptance

1. `zygo decision new` writes a file that `check` reports as malformed until the hint lines are replaced; after that `check` exits 0.
2. Two records created at the same time in two linked checkouts get different ids; merging both branches gives no conflict.
3. A record with a cycle in `depends_on`, a missing `depends_on` id, a file name that differs from `id`, or a 65 KiB file makes `check` exit 1 with one line each; `ls` exits 0 and names each file on stderr.
4. `deps` on an assumption with a chain of 3 records and one record reached on two paths prints 3 lines with depths 1, 2, 3 and the shared record once.
5. With the guard at `enforce`, `work_verdict` keep is refused for: no `head`; no `dispositions`; a duplicate label; a `fixed` commit that is not an ancestor of `head`; `recorded` in round 2; `recorded` with class `security`; a record named but absent at `head`; a node commit that changes a record to `ratified`; a node commit that deletes a record; a new record with class `contract`. Each refusal names the problem, and the row's verdict is unchanged.
6. With the guard at `enforce`, a keep with one `fixed`, one `rejected` and one `recorded` (round 3, class `none`, valid record at `head`) is recorded.
7. With the guard at `monitor`, each case of item 5 records the verdict, the reply is the current reply, and no git process runs. At `notify` the verdict is recorded and the reply ends with the problems.
8. A keep whose git calls pass their bound is refused at `enforce` with `cannot check:`; no hook output changes. A keep whose `head` is already in `HEAD` is recorded at `enforce` when checks 1 to 7 pass, and the reply says that record changes were not checked.
9. `zygo work report` prints `decisions: 2 open, 3 total` in the unit block and `decision records: 2 open, 3 total, deepest chain 2, 1 malformed` for a fixture repository; outside a repository it prints no records line.
10. `review` refuses to run with `ZYGO_SESSION_DIR` set and without a terminal. It shows records in dependents order, largest first. After an overturn, the open dependents of that record come next. Each outcome is in its file before the next prompt; a second `review` shows only the records still open.
11. A record changed on disk while `review` shows it is skipped, and the file is unchanged. A record title with an ESC character prints as `\u{1b}`.
12. `ratify --promote rule --as <text> --yes` writes `status`, `reviewed`, `promoted_to`, `promotion`, and keeps unknown keys, comments and the body byte-equal.
13. No record text, finding label, reason or disposition is in the usage store, a log or the session directory.

## 18. Premises the first build verifies

- **PD1** `git merge-base --is-ancestor`, `git cat-file -s` and `git show <commit>:<path>` in the main checkout read commits made in a linked checkout of the same repository, before any merge.
- **PD2** Claude Code passes a `work_verdict` argument object of 256 KiB to the MCP server unchanged.
- **PD3** On Claude Code 2.1.328 the `PreToolUse` input for `work_verdict` from the main loop has no `agent_id` key, so the existing lead-only hook lets the signature through (the deny branch at `crates/zygo-work/src/hooks.rs:184-185`).
