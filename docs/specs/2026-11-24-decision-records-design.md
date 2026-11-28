# zygo: delegated decision records — design

**Status:** approved by the operator 2026-11-24
**Date:** 2026-11-24
**Release:** phase 3, track A (roadmap items P3-8, P3-9 and P3-10, `docs/roadmap/phase-3.md`)
**Base specs:** `docs/specs/2026-11-18-zygo-design.md` (`§n`). Also cited: `docs/specs/2026-11-20-workspaces-design.md` (`W§n`, decisions `AUn`), `docs/specs/2026-11-22-workspace-mail-design.md` (`M§n`). `D§n` is a section of this document.

Each decision has a mark. **(operator)**: the operator made the decision (the roadmap's P3-8 and P3-10 design notes, the P3-9 item row, and the plan of work of 2026-11-24). **(lead)**: the lead of this design work ruled it after an independent audit of the first draft (2026-11-24). **(author)**: the author made it; the operator can change it before approval. Code references are `file:line` at commit `6e7fe2f`. Skill references are to the act-lite copy of the skills at `plugins/act-lite/skills/` in a zygo session snapshot (§15.3); the act101 plugin carries the same skills.

---

## 1. Intent

The `zygo-<role>` subagents make decisions and assumptions that the operator did not make. A review gate runs 3 rounds at most. Today a finding that is open after round 3 needs the operator, so the work stops.

The operator's direction (2026-11-23): each such decision gets a record, in the same way as an architecture decision record (ADR), but after the fact. The work then passes the 3-round gate and continues with no operator action. The operator audits the records at the end of a phase and makes categorical changes when an assumption was incorrect. No finding stays without a record and a disposition **(operator)**.

This spec settles the record (P3-8), what zygo enforces (P3-9) and the phase-end audit (P3-10).

Today zygo sees none of this. `work_verdict` accepts a `findings` string and drops it: "`findings` is accepted, not stored (§14 stores no free text)" (`crates/zygo-mcp/src/tools/work.rs:58`). It stores only the verdict word on the unit row (`crates/zygo-usage/src/store.rs:288-297`).

## 2. Goals

- **D-G1** Each decision an agent makes under delegation, and each review finding it does not fix, has one durable record in the repository, in the pull request that caused it.
- **D-G2** Each review finding has exactly one disposition: fixed, rejected or recorded.
- **D-G3** A node passes its review gate with no operator action, unless a finding or a record is of a class that stops for the operator. A stopped node passes after the operator ratifies its stop-class records (D§8.5).
- **D-G4** One query lists each record that depends on a given assumption, directly or through other records.
- **D-G5** At the end of a phase the operator sees each open record, largest dependency set first, and ratifies, overturns or promotes it.
- **D-G6** zygo refuses a `keep` verdict that breaks the disposition rule. The refusal is on by default (D§9.3).

## 3. Non-goals

- A numeric limit on open records. Phase 3 measures the count and the dependency depth; the first phase-end review decides if a limit is necessary **(operator)**.
- Work graph edits by zygo. zygo never writes `graph.md` or a node file. An overturn makes a work node through the planning skill (D§12).
- A security boundary against the agents (D-C9).
- A check that the lead lists every finding of a review. zygo does not see the reviewer's text (C3, §14). The lead signs the list (D§8.4).
- Records in the usage store, or any record text in a zygo store or log.
- The lead-only rule of `work_verdict` on OpenCode. It is not enforced there today (§8.7 note: no `work_verdict` hook, no agent id), and this spec does not change that. The record files and the verdict check work on OpenCode.

## 4. Constraints

- **D-C1** C3 and §14 hold. zygo stores no record text, finding text or disposition reason in the usage store, a log or a session file. The record files are repository content that agents write, as code is.
- **D-C2** C6 holds. No hook reads a record file, and the slash form reads none (D§10.1). The verdict check is in the MCP tool, which can refuse; it is not a hook.
- **D-C3** The record file is the one owner of a record's state. zygo stores no copy and no index. Each derived value (dependents, depth, counts, the node range) is computed at each use.
- **D-C4** A record is never deleted or renamed. A record leaves the open set only by a status change (D§6.6).
- **D-C5** An agent never writes an operator field or an operator status (D§6.6). Only the operator verbs (D§11) write them.
- **D-C6** Every new input is bounded: tool arguments (D§9.2), record files and the directory (D§6.8), the git reads of one verdict (D§9.4), and each git process (2 s, as the §13.5 note).
- **D-C7** The work-unit rules of §13 hold. The new guard (D§9.3) follows the §13.4 levels and the §13.5 resolution.
- **D-C8** The skills are outside this repository. This spec names the skill changes (D§12); the plugin makes them **(operator)**.
- **D-C9** Threat model **(lead)**. The checks of this spec stop mistakes by an agent that follows its instructions. They are not a security boundary against an agent that runs as the same OS user: such an agent can unset `ZYGO_SESSION_DIR`, run an operator verb, commit without a `Zygo-Session:` trailer, or commit on the work branch through its shell, which no guard blocks (§8.3, known limit). No sentence of this spec claims more.

## 5. Terms

| Term | Meaning |
|---|---|
| DDR | Delegated decision record. The agent decided under delegated authority and the operator has not ratified the decision. "PDR" is not used: it is a known term (preliminary design review) **(operator)**. |
| Record | One DDR file under `docs/decisions/` (D§6). |
| Finding | One defect that a reviewer reports in one review round of one node, with its location, the failure and the evidence (execute-work `SKILL.md:146-158`). |
| Disposition | The end state of one finding: `fixed`, `rejected` or `recorded` (D§8.1). |
| Stop class | A class of finding or record that stops the node for the operator (D§8.2). |
| Dependent | A record B is a dependent of record A when A is in B's `depends_on`, or in the `depends_on` of a dependent of A. |
| Signer | The session that submits the dispositions: the lead, through `work_verdict` (D§8.4). |
| Operator verb | `zygo decision ratify`, `overturn` or `review` (D§11). |
| Unit | The work unit id. For a node of a work graph it is `<program>:<node id>`, the form the skill writes in each handoff (execute-work `SKILL.md:245`), for example `phase-3:N12`. |
| Node range | The commits `base..head` of one verdict (D§9.4 step 4): the node's own work. |
| Agent commit | A commit in the node range with one parent and a `Zygo-Session:` line in its message. §8.6 item 7 tells every agent and subagent to add that trailer; the operator's own commits do not have it (D§8.5). |

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
unit = "phase-3:N12"
round = 2
role = "code"
model = "claude-sonnet-4-5"
commit = "6e7fe2f0c1d2e3f4a5b6c7d8e9f0011223344556"
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
| `kind` | `assumption`, `decision`, `accepted-limit`, `compromise`, `open-finding` | agent, at creation; frozen | D§6.4. |
| `status` | `open`, `ratified`, `overturned`, `fixed`, `superseded` | agent or operator, by D§6.6 | D§6.5. |
| `class` | `none` or a stop class of D§8.2 | agent, at creation; frozen | The stop class of the subject, else `none`. |
| `title` | one line, 1 to 120 bytes | agent | Short name for lists. |
| `unit` | unit id (D§5), at most 128 bytes, §13.1 characters | agent, at creation; frozen | The work unit that made the decision. |
| `round` | integer ≥ 1 | agent, at creation; frozen | The unit's round (§13.1). |
| `role` | a role name of the session (`code`, `lead`, …) | agent, at creation; frozen | The role that made the decision. `lead` when the decision is in the lead's brief. |
| `model` | catalog id, or `unknown` | `zygo decision new` (D§10.2) | The model that served the role. |
| `commit` | 40 or 64 hex | `zygo decision new` | `HEAD` of the checkout when the record was written: the code the decision was made against. |
| `date` | `YYYY-MM-DD` | `zygo decision new` | Creation date, UTC. |
| `depends_on` | array of ids, at most 64 | agent | The records this one builds on: the assumptions it depends on, and any other record. |
| `reverse_cost` | `low`, `medium`, `high` | agent | `low`: one node or less. `medium`: more than one node, and no public contract or stored format. `high`: a public contract, a stored format, a migration or data. |
| `supersedes` | one id, optional | agent, at creation | The record that this one replaces (D§6.6). |
| `fixed_by` | 40 or 64 hex | agent | The commit that fixed the subject. Required when `status = "fixed"`. |
| `promoted_by` | 40 or 64 hex | agent | The commit that wrote the promotion (D§11.3). |
| `reviewed` | `YYYY-MM-DD` | operator verb | Date of the operator's outcome. |
| `review_note` | text, at most 2,000 bytes | operator verb | The operator's reason. Required for `overturned`. |
| `promoted_to` | `adr`, `spec`, `rule` | operator verb | D§11.3. |
| `promotion` | text, at most 500 bytes | operator verb | Where the promotion goes and what it says. Required with `promoted_to`. |
| `## Finding` | Markdown section | agent | The reviewer's finding, with its location, failure and evidence. Required for `accepted-limit`, `compromise` and `open-finding`; also written, but not checked, in a `decision` record made from a stop-class finding (D§8.2). |
| `## Choice` | Markdown section, not empty | agent | The choice. For `open-finding`: what stays open. |
| `## Alternatives rejected` | Markdown section, not empty | agent | For `open-finding`: the fixes considered, and why none was made. |
| `## Evidence` | Markdown section, not empty | agent | |
| `## Cost to reverse` | Markdown section, not empty | agent | The detail of `reverse_cost`. |

"Frozen" means no agent commit changes the key after creation (D§9.4 step 9). Reason **(lead)**: a change of `class` to `none` would take a record out of the stop rule, and a change of `kind` would make a finding record into a decision record after the gate read it.

**(author)** "The later records that build on it" is not stored. It is the inverse of `depends_on`, computed at each read (D-C3). Reason: a stored list is a second owner of one fact. Two parallel nodes that each add a dependent would both edit the earlier record, and their merges would conflict; a list edited by hand drifts from the `depends_on` keys. `zygo decision show` and `deps` print it (D§10).

Unknown keys are kept and ignored. Unknown sections are kept.

### 6.4 Kinds

**(operator)** One record type, with a kind. **(author)** The meaning of each kind:

| Kind | Use |
|---|---|
| `assumption` | A fact the agent took as true without proof (an API behavior, a premise about the user). Other records cite it. |
| `decision` | A choice between alternatives that the spec or the brief did not make. Also: a finding the author holds is not a defect, when the finding is of a stop class (D§8.2). |
| `accepted-limit` | A confirmed finding that stays as it is, because the limit is acceptable. |
| `compromise` | A confirmed finding fixed in part, or fixed by a means other than the one the spec states. |
| `open-finding` | A confirmed finding that is not fixed and not accepted: a defect left for the operator. |

### 6.5 Statuses

**(operator)** `open`, `ratified`, `overturned`, `fixed`, `superseded`.

| Status | Meaning |
|---|---|
| `open` | Written by an agent. The operator has not decided it. |
| `ratified` | The operator accepted it. With `promoted_to`: accepted and promoted (D§11.3). |
| `overturned` | The operator rejected it. A work node reverses it (D§11.3). |
| `fixed` | A commit fixed its subject (`fixed_by`). |
| `superseded` | A later record replaced it (that record's `supersedes`). |

### 6.6 Transitions

**(author)** Only these transitions are allowed. Each one is made in a commit that carries the trailer of D§6.7.

| From | To | By | Writes |
|---|---|---|---|
| none | `open` | agent | A new file. Operator keys absent; `fixed_by` and `promoted_by` absent. |
| `open` | `open` | agent | Agent keys that are not frozen, and sections. Operator keys stay absent. |
| `open` | `fixed` | agent | `status`, `fixed_by`, a commit in the node range. |
| `open` | `superseded` | agent | `status`. A record added in the same node range names it in `supersedes`. |
| `overturned` | `fixed` | agent | `status`, `fixed_by`, a commit in the node range (the commit that carried out the reversal). |
| `ratified` with `promoted_to` and no `promoted_by` | the same | agent | `promoted_by` only, a commit in the node range. |
| `open` | `ratified` | operator verb | `status`, `reviewed`, `review_note` (may be empty); with promote also `promoted_to`, `promotion`. |
| `open` | `overturned` | operator verb | `status`, `reviewed`, `review_note` (not empty). |
| `ratified` without `promoted_to` | `overturned` | operator verb | As above. The operator can change an outcome. |

`fixed` and `superseded` are final. An agent never changes a `ratified` record except for `promoted_by`, and never makes a record `ratified` or `overturned` (D-C5). D§9.4 step 9 checks this for agent commits. The mistake it stops: an agent that ratifies its own record removes it from the operator's audit, and the audit is the purpose of the track. Within D-C9 it is a check on mistakes, not a lock.

`overturn` of a promoted record is refused (D§11.3) **(lead)**. Reason: a promotion has made, or will make, an ADR, a spec text or a rule; an overturn would leave that text in force. The operator first removes `promoted_to`, `promotion` and `promoted_by` by hand, with the promoted text, and then overturns.

### 6.7 The `Zygo-Decision:` trailer

**(operator)** P3-9 adds a commit trailer. **(author)** Form: `Zygo-Decision: <id>`, one line per id, at the end of the message with the other trailers (§13.4). A commit carries it when it:

1. adds or changes a record;
2. carries out a decision (the code that a record of kind `decision` or `compromise` describes);
3. fixes the subject of a record (`fixed_by`), or writes a promotion (`promoted_by`).

Use: `git log --grep '^Zygo-Decision: <id>$'` lists each commit made under a record. An overturn needs this list: it is the set of changes to reverse. The commit that adds a record file is also found by `git log --diff-filter=A -- <file>`; the trailer is for item 2, which does not touch the record file.

No guard checks the trailer (D§9.6).

### 6.8 Limits

**(author)** The directory: a reader `lstat`s `docs` and `docs/decisions`. Absent is not an error (D§14). A reader does not enter subdirectories. Two cases refuse the directory, each with its own line:

- A link or a file that is not a directory: the listing verbs (`ls`, `show`, `deps`, `check`), `new` and the operator verbs (`review`, `ratify`, `overturn`) exit 1 with `zygo decision: docs/decisions is not a directory`, and the report prints `decision records: docs/decisions is not a directory`.
- A directory that cannot be read: an `lstat` that fails for a reason other than absent, a listing that cannot be opened (for example, no read permission), or an entry that gives an error while the listing is read. The same verbs exit 1 with `zygo decision: cannot read docs/decisions`, and the report prints `decision records: cannot read docs/decisions`. This case is never reported as not a directory, and no part of the listing is used.

The names: a file is a record when its name matches `^DDR-[0-9A-HJKMNP-TV-Z]{26}\.md$`. A name that starts with `DDR-` and ends in `.md` but does not match is malformed. Every other name (`README.md`, `*.tmp`, other files) is not a record and is ignored. A reader considers at most 10,000 entries of the directory. Above that, a listing verb exits 1 with `zygo decision: more than 10,000 files in docs/decisions/`, and the report prints `decision records: over 10,000 files`.

A record file is valid when all of these hold; else it is malformed:

- a regular file (`lstat`; a link, a FIFO or a directory is never opened), at most 64 KiB, UTF-8;
- line 1 is `+++`, a second `+++` line follows within the first 8 KiB, and the text between them parses as TOML;
- each required key is present with its D§6.3 type and limit; `id` equals the file stem;
- the four required sections exist and are not empty, and `## Finding` exists for `accepted-limit`, `compromise` and `open-finding`. A section that still holds the marker `<!-- zygo: fill -->` (D§10.2) counts as empty;
- the status keys agree: `fixed` has `fixed_by`; `overturned` has a non-empty `review_note`; `promoted_to` has `promotion`.

Set checks (D§10.1 `check`): ids are unique; each `depends_on` and `supersedes` id names a record; `depends_on` has no cycle; the target of `supersedes` has status `superseded`.

In git (D§9.4), the same rules apply to a blob read at a commit. A tree entry that is a link or a submodule is not a regular file, so it is malformed. A path under `docs/decisions/` whose name is not a record name, or that is in a subdirectory, is not a record and is ignored.

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

**(author)** The act-lite verification stage marks each finding confirmed or refuted (execute-work `SKILL.md:159-161`). A refuted finding is `rejected`, with the evidence as the reason. A confirmed finding is `fixed`, or `recorded` with a record of kind `accepted-limit`, `compromise` or `open-finding`. A stop-class finding follows D§8.2.

**(lead)** Each proof is bound to the node. A `fixed` commit is in the node range, not any older commit. A `recorded` record was added in the node range, has the verdict's unit and the entry's class, and serves one finding only. Reason: a commit or a record from earlier work proves nothing about this node's finding.

**(operator, plan of work 2026-11-24, decision 3)** Each work node has its own review gate of up to 3 rounds. After round 3, a node passes only when each finding has exactly one disposition, and no finding is of a stop class.

**(author)** `recorded` of class `none` is accepted only in round 3 or later. In rounds 1 and 2 a confirmed finding is fixed, or the round fails and a new round starts. Reason: the operator's direction is that a finding open *after round 3* gets a record. Without this limit an agent can record each finding in round 1, skip the review loop, and move the work to the operator's audit. A finding that is not a defect, and not of a stop class, is `rejected` with its reason in any round. A stop-class finding can be recorded in any round: it stops for the operator anyway (D§8.2), so recording it skips no review, and an early stop saves rounds that cannot fix it.

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

**(operator: P3-9 row and plan of work decision 3; lead ruling)** The rule applies to findings and to records:

- A finding of a stop class stops the node when its disposition is `recorded` or `rejected`. To pass, it is `recorded` with a record of the same class, and the operator ratifies that record (D§8.5). A finding the author holds is not a defect is recorded with kind `decision`, which says why.
- A record of a stop class added in the node range stops the node, whatever its kind, until the operator ratifies it. An agent that decides alone to change a CLI flag cannot record that and continue.
- **(author)** A stop-class finding `fixed` by a commit in the node range does not stop the node. Reason: the P3-8 note forbids recording and continuing; a fix is not a record, and it removes the defect. The fix is in the node range, so it is in the pull request diff. The deliver step lists each fixed stop-class finding in the pull request body (D§12), so the operator sees each one.

### 8.3 What an author decides alone

**(author)** Without a record: mechanics (structure, order, internal names, how to test, how to fix a finding: execute-work `SKILL.md:55-56`). With a record of class `none`: a choice the spec and the brief leave open, an assumption, an accepted limit, a compromise, an open finding. With a record and the operator: a stop class (D§8.2, D§8.5). The plan step writes this into the graph header (D§12).

### 8.4 Who signs

**(operator)** The author does not accept its own compromise. The reviewer or the lead signs the disposition.

**(author)** The signature is the `work_verdict` call. Only the lead can make it on Claude Code: the `PreToolUse` hook denies it in a subagent (`crates/zygo-work/src/hooks.rs:183-186`, text at `:164`; hook registered at `crates/zygo-launch/src/claude_code.rs:484`). The implementer and the reviewer are subagents, so neither can sign.

The act-lite skills call the lead "the author" when a subagent writes text from the lead's brief (execute-work `SKILL.md:49-54`). Thus a decision that is in the lead's brief (`role = "lead"`) is the lead's own. For such a record the reviewer signs: the round-3 reviewer states in its report, for each finding, whether it accepts that the finding is recorded if it is not fixed. The lead records a finding of a `role = "lead"` decision only when the reviewer accepted it. zygo cannot see the reviewer's text (D-C1), so this part is a skill duty (D§12).

### 8.5 The operator path for a stopped node

**(lead, form by the author)** A stopped node passes only after the operator acts. The flow:

1. The lead calls `work_verdict` `keep`. The gate refuses with one problem per stop item (D§9.4 steps 6, 7 and 8), each naming the record id. A stop-class finding that is `rejected`, or not yet recorded, gives `<label>: class <class> needs a record for the operator`.
2. The skill sets the node `BLOCKED(stop: <class> <ids>)`. It keeps the node's branch and checkout (it does not discard them, D§12), and asks the operator. The question names each record id and the checkout path.
3. The operator, in an own shell (not an agent session, D§11.1), in the node's checkout (or the one checkout of a one-checkout workflow), runs `zygo decision ratify <id>` or `zygo decision overturn <id> --reason <text>` for each record, and commits with plain `git commit`. The commit has no `Zygo-Session:` line, so it is not an agent commit.
4. The operator tells the lead to continue. The lead calls `work_verdict` `keep` again with the new `head`.
5. The gate reads each stop-class record at `head`. `ratified`: the item passes. `overturned`: the problem `<label>: <id> overturned by the operator`; the node starts a new round, fixes the subject in the node range, and sets the record `fixed` (D§6.6), and the finding becomes `fixed`. The operator's commit is in the node range but is not an agent commit, so step 9 does not count its operator fields.

Rejected form: the operator ratifies on the work branch. The record is a new file of the node branch. A copy ratified on the work branch is a second added file with other content, so every later merge between the two branches stops on an add/add conflict of that file.

Within D-C9, step 3 is not proof that the operator acted. An agent that commits without the trailer passes the same check. The trailer rule of §8.6 item 7 and the `trailers` guard (§13.4) are what make this a check on mistakes.

## 9. Enforcement in zygo (P3-9)

### 9.1 Data path

**(author)** zygo learns the findings and their dispositions from the `work_verdict` call itself, and the records from the git commits the call names. No new store.

1. The reviewer reports findings to the lead (skill).
2. After the round-3 review, the node's implementer writes the records in a records step (D§9.7) and commits them with the trailer (D§6.7). The records are in `head`.
3. The lead calls `work_verdict` with `verdict = "keep"`, `head` (the node branch's head commit), optional `base`, and one `dispositions` entry per finding. The lead calls it **before** the merge into the work branch (D§12): the call is the gate.
4. The tool checks the entries against the node range (D§9.4). Git objects are shared by all linked checkouts, so the lead's repository reads the node's commits.
5. The durable copy of the disposition list is the node file's `## Review` section and the pull request body (skill, D§12). zygo keeps no text (D-C1).

A file written by the reviewer was rejected: `zygo-review` has no edit tool (`writes = false`, `crates/zygo-config/data/roles.toml:27-31`, so `disallowedTools`, §8.5), and the lead cannot edit (§8.3). A trailer was rejected: a disposition list with reasons does not fit trailers, and a finding fixed in round 1 has no commit of its own.

### 9.2 `work_verdict` input

**(author)** Three optional keys are added to the input schema (`crates/zygo-mcp/src/tools/work.rs:50-55`). `findings` stays a string and keeps its meaning. `verdict` keeps its three values (`work.rs:16`).

```json
"head": {"type": "string", "pattern": "^[0-9a-f]{7,64}$"},
"base": {"type": "string", "pattern": "^[0-9a-f]{7,64}$"},
"dispositions": {"type": "array", "maxItems": 256, "items": {"type": "object", "properties": {
  "finding":     {"type": "string", "minLength": 1, "maxLength": 200},
  "class":       {"type": "string", "enum": ["none","acceptance","security","irreversible","contract","spec","rule"]},
  "disposition": {"type": "string", "enum": ["fixed","rejected","recorded"]},
  "commit":      {"type": "string", "pattern": "^[0-9a-f]{7,64}$"},
  "reason":      {"type": "string", "minLength": 1, "maxLength": 2000},
  "record":      {"type": "string", "pattern": "^DDR-[0-9A-HJKMNP-TV-Z]{26}$"}
}, "required": ["finding","class","disposition"]}}
```

`finding` is the lead's short label for the finding (for example `R2-3 unbounded read in report`). `base` is for a one-checkout workflow: the last commit before the node's work (D§9.4 step 4). The description gains one sentence: "On keep, give `head` (the commit you judged) and `dispositions` (an empty list when the review had no findings): one entry for each review finding, fixed with its commit, rejected with its reason, or recorded with its DDR id; in one checkout, also give `base`."

**Argument refusals at every level** **(author)**. The tool validates the arguments itself, as it does today for `unit`, `round`, `verdict` and `findings` (`work.rs:60-75`). These refusals are `InvalidParams` and do not depend on the guard level: a key of the wrong type; a value outside its enum, pattern, length or item limit; an entry with a key that its disposition does not take (`commit` only on `fixed`, `reason` only on `rejected`, `record` only on `recorded`); an argument object over 256 KiB. The absence of `head`, `base` or `dispositions` is not an argument refusal: it is a guard problem (D§9.4), because the three keys are optional for `discard` and `rework`.

### 9.3 The `dispositions` guard

**(operator: P3-9 row, "the verdict step refuses a pass while a finding has no disposition"; lead ruling)** The check is on by default and refuses. It is a guard of §13.4, so its level comes from config, `.zygo.toml` and the session override, with the `min_level` floor (§13.5).

| Guard | Trigger | Event when | notify text / enforce action |
|---|---|---|---|
| dispositions | `work_verdict` with `verdict = "keep"` | any problem of D§9.4 | the verdict is recorded and the reply ends with `zygo: dispositions: <problems>` / the verdict is not recorded; the tool returns an error with `zygo: keep refused: <problems>` |

- Default level: `enforce`. The other work guards default to `monitor`, except `trailers`, which defaults to `notify` (`crates/zygo-config/src/schema.rs:540-551`). The new default is added there.
- At `monitor` the check does not run and none of the four verdict git processes of D§9.4 starts: the guard has no place to record an event (next item), and the tool behaves as today. Reading the level (the config load, including the `.zygo.toml` read from the default branch, §13.5) runs at every level, `monitor` included, and is outside the D§9.4 budget. A user or a repository can set this level, as for any guard, above the `min_level` floor.
- The guard runs in the MCP tool, not in a hook. It records no `guard` line and no count in `guard_events`: that column is written by the fold, which replaces it on each fold (`crates/zygo-usage/src/store.rs:252`), so the tool cannot own a part of it. The reply is the only output.
- A `keep` without `dispositions` is refused at `enforce`. Today's skill calls `work_verdict` without it (execute-work `SKILL.md:224-230`), so the skill change of D§12 ships before a zygo release with this guard. Until a binary with this item is installed, the skill applies the rule (plan of work, decision 3).
- `<problems>` is at most 10 problems joined by `; `, then `; and <n> more`, at most 4 KiB.

### 9.4 The checks

**(author)** The tool runs these steps in order and collects each problem. Git runs in the session's repository root (D§13 item 6). The check of one verdict starts at most four git processes: one `git cat-file --batch-command` for all object reads, one `git merge-base`, one `git rev-list` and one `git log`. Reading the level (D§9.3) is outside this budget and runs at every level. Each process has a 2 s bound from its start, and all of them together a 10 s bound (D-C6). `merge-base`, `rev-list` and `log` run first, with `head` and `base` as given (git resolves them as `<rev>^{commit}` does; their answers are not used when step 1 or 4 refuses the rev); the `--batch-command` process starts after them, at the first object read, so its 2 s covers every read of steps 1 to 9. Problems are still given in step order. A commit is read with `contents`. `<rev>:docs/decisions` is first asked with `info`: when it is not a tree, it is missing and its contents are never read; a tree is then read with `contents`. A record file is first asked with `info`, which gives its size and no contents: a file over 64 KiB is a malformed record and its contents are never read (D§6.8, D§14); a file of at most 64 KiB is then read with `contents`. Each blob is asked once per verdict, and each `docs/decisions` tree once per commit. The process reads at most 2,048 objects with `contents` and 16 MiB of contents in all; past either bound the problem is `too much to read`. `info` requests count toward neither bound: each one comes before a `contents` read or answers a tree or record that the 1,000 record changes of step 9 bound. A step that needs a failed read gives its problem and the later steps that need the same read are skipped.

1. **Head.** `head` is present and `<head>^{commit}` resolves (`--batch-command`) to a commit whose full id starts with `head`. Else: `no head commit` when `head` is absent or does not resolve, and `cannot check: head <head> is a ref name, not a commit id` when the resolved id does not start with `head`: git tries refs before an object prefix, so a ref of the same name resolved instead, and the check cannot know which commit the lead judged.
2. **Dispositions.** `dispositions` is present. An empty array is valid (no findings). Else: `no dispositions`.
3. **Uniqueness.** Each `finding` label is unique, and each `record` id is in one entry only. Else: `finding <label> has two dispositions` or `<id> serves two findings`.
4. **Node range.** With `base`: `<base>^{commit}` resolves (`--batch-command`) to a commit whose full id starts with `base`, else `no base commit` when it does not resolve, and `cannot check: base <base> is a ref name, not a commit id` when the resolved id does not start with `base`; and `git merge-base <base> <head>` must equal `base`, else `base is not an ancestor of head`. Without `base`: base is `git merge-base HEAD <head>`; when it equals `head`, the problem is `head is already in HEAD: call before the merge, or give base` and steps 5 to 9 are skipped. Then `git rev-list <base>..<head>` lists the node range R, at most 10,000 commits (else `node range too large`).
5. **`fixed`.** `commit` is present, else `<label>: no commit`. `<commit>^{commit}` resolves to a commit whose full id starts with `commit`, else `<label>: cannot check: commit <c> is a ref name, not a commit id` when the resolved id does not start with `commit`. That full id is in R. Else: `<label>: commit <c> is not in the node range`.
6. **`rejected`.** `reason` is present. When `class` is not `none`: `<label>: class <class> needs a record for the operator` (D§8.2).
7. **`recorded`.** All of these, else `<label>: <why>`:
   - with class `none`, the verdict's `round` is 3 or more (D§8.1);
   - `<head>:docs/decisions/<record>.md` is a valid record (D§6.8) and `<base>:docs/decisions/<record>.md` is missing: the record was added in the node range;
   - its `unit` equals the verdict's `unit`, and its `class` equals the entry's `class`;
   - its kind is `accepted-limit`, `compromise` or `open-finding`; or `decision` when the class is not `none` (D§8.2);
   - each id in its `depends_on` is a record at `head`;
   - its status: with class `none`, `open`; with a stop class, `ratified` passes, `open` gives `class <class> stops for the operator: ratify or overturn <id>`, and `overturned` gives `<id> overturned by the operator`.
8. **New stop-class records.** `git log --no-renames --diff-merges=first-parent --name-status -z --format=<id, parents, message> <base>..<head> -- docs/decisions/` gives the record paths that R touches. Each record at `head` that is missing at `base` and has a class other than `none` must be `ratified` or `fixed` (else `<id>: class <class> stops for the operator: ratify or overturn <id>`). This covers records that no finding names. `fixed` passes for the reason of D§8.2: the subject was removed by a commit in the node range (step 9 checks `fixed_by`); it is the end of the overturn flow of D§8.5 step 5.
9. **Agent transitions.** For each agent commit (D§5) in the same `git log` output, and each record path it changes, the old version (at its parent) and the new version are read (`--batch-command`). The change must be an agent transition of D§6.6: no frozen key changed; no operator key or operator status written; `fixed_by` and `promoted_by` in R; for `superseded`, a record added in R names it. A deleted or renamed record, a malformed new version, or a malformed name is a problem. At most 1,000 record changes are read (else `too many record changes`). Merge commits and commits without `Zygo-Session:` are not read: they are not the node's own agent work.

No other files are read. A malformed record that the node did not touch is not the node's problem; `zygo decision check` reports it.

A git process that fails or passes its bound gives the problem `cannot check: <git step> <why>`. At `enforce` the keep is refused, and the lead can call again. Reason: this is the gate, and a check that passes on error does not gate. It is not a hook, so C6 does not require the call to pass (D-C2).

### 9.5 `zygo work report`

**(operator)** `zygo work report` counts open records. **(author)** The output of `crates/zygo-work/src/report.rs` gains two lines. Records are read from the working tree of the repository at the current directory (`git rev-parse --show-toplevel`, 2 s bound).

- In each unit block, after the `guard events:` line (`report.rs:82`), when at least one valid record has this `unit`, or this unit with a `:<part>`: `  decisions: <open> open, <total> total`.
- After the last block, after one empty line, when `docs/decisions/` holds at least one record name: `decision records: <open> open, <total> total, deepest chain <d>`, then `, <m> malformed` when `m > 0`. `<d>` is the number of records on the longest `depends_on` path among valid records (a record with no `depends_on` counts 1; a cycle is cut at the first repeat). The three D§6.8 directory cases (not a directory, cannot be read, over 10,000 files) print their own line in place of this one.
- With no units and at least one record, the report prints `no work units` and then the records line.
- Outside a repository, or when the git call fails: no records lines. The report never fails because of a record.

### 9.6 No guard on the trailer

**(author)** No guard checks `Zygo-Decision:`. The trailers guard reads the message text only (`crates/zygo-work/src/guards.rs:111-115`, `:1149-1152`). To check that a commit that adds a record carries the trailer, the hook would run `git diff --cached` on each `git commit`, and the forms `git commit -a` and `git commit <paths>` change the file set. The mistake it would stop is small: the record's commit is found by its path (D§6.7). The trailer that matters (item 2 of D§6.7) touches no record file, so no hook can tell that it is missing.

### 9.7 Who writes the records

**(lead, form by the author)**

- **Records from findings.** After the round-3 review, the node's implementer runs a records step in the node's checkout, under the node's unit (`<program>:<node id>`) and round 3. It writes one record per finding that the lead will record, with `zygo decision new`, and commits them with the trailer. The skill puts `docs/decisions/` in the handoff's `Files:` line. Reason for the directory and not `docs/decisions/DDR-*.md`: the scope guard matches a listed path or a `dir/` prefix only (§13.3), so a pattern would make each record write a scope event.
- **Assumption and decision records.** The implementer writes them during the node's work, in any round (D§8.3), under the same unit.
- **What the reviewer sees.** The round-3 reviewer reviewed before the records step, so it does not see the records from findings. It sees each assumption and decision record that is in the diff it reviews. The gate (D§9.4) checks the records from findings mechanically, and the operator reads them at a stop (D§8.5) or at the phase end (D§11).

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

`new`, `ls`, `show`, `deps` and `check` exit 2 outside a git repository. `show` and `deps` exit 1 when `<id>` is not a valid record. Promote is `ratify --promote`, because a promotion is a ratification with a target (D§6.5); `--promote` without `--as` exits 2. `ratify` of a record that is not `open`, and `overturn` of a record that is not `open` or is not `ratified` without `promoted_to`, exit 1 with `zygo decision: <id> is <status>` (or `zygo decision: <id> is promoted; remove the promotion by hand first`), and change nothing.

Text from a record (title, body, notes) and from git (commit subjects) is printed one line at a time through `zygo_core::esc` (`crates/zygo-core/src/text.rs:4`), so an agent-written record cannot send terminal escapes to the operator's terminal. `review` uses the same rule.

**Slash form** **(lead)**. `/zygo:decision <args>` replies `run in your shell: zygo decision <args>` for every subcommand and reads no file. Reason: the slash form runs inside the `UserPromptSubmit` hook (§17.1), and D-C2 keeps record reads out of hooks.

### 10.2 `zygo decision new`

**(author)** Fills `id` (a new ULID), `status = "open"`, `date`, `commit` (`git rev-parse HEAD`), and `model`: inside a session, the `model` of the newest `requests` row of this session whose `role_served` is `<r>` (§14); else `unknown`. In serve mode the rows reach the local store only at exit (§14), so it writes `unknown`. The arguments are validated: `--unit` by the D§6.3 limit, `--round` ≥ 1, `--role` a role of the session's legend or `lead` (outside a session: `^[a-z][a-z0-9-]{0,31}$`), each id by D§6.1 and present as a valid record.

It writes the front matter and the four (or five, D§6.3) section headings, each with a one-line hint for the agent to replace, in `docs/decisions/<id>.tmp`, then renames it to `<id>.md`. It creates `docs/decisions/` when absent. It does not commit. Each hint line is `<!-- zygo: fill --> <hint>`. A write failure leaves at most the `.tmp` file, which every reader ignores; the verb exits 1.

Reason for the verb: a record written by hand gets a bad ULID, a wrong key name or a missing section, and the error shows only at the verdict. The hint lines make an unedited record malformed (`check` fails on them), so a template that the agent forgot to fill cannot pass.

## 11. Phase-end ratification (P3-10)

### 11.1 Who can run it

**(author)** The operator verbs (`ratify`, `overturn`, `review`) are refused, exit 2, when `ZYGO_SESSION_DIR` is set, with `zygo decision <verb>: run it in your own shell, not in an agent session`. This is the rule for code approval (§17.1, N30 note). `review` also needs a terminal on stdin and stdout; else exit 2 with `zygo decision review: needs a terminal; use ratify or overturn with --yes in scripts`. `ratify` and `overturn` show the record and ask `[y/N]`, unless `--yes`.

**(lead)** This refusal and D§9.4 step 9 stop an agent that runs an operator verb by mistake. They do not stop an agent that unsets the variable, commits without the trailer, or commits on the work branch through its shell (D-C9). The operator's audit at the phase end, and the record list in the pull request body (D§12), are where such a change shows.

The operator verbs act on the working tree of the current directory: the work branch's checkout at the phase end, or the node's checkout for a stop (D§8.5).

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
| Overturn **(operator)**: this makes a work node | the reason (not empty) | `status = "overturned"`, `reviewed`, `review_note` | The planning skill makes a node that reverses the commits of D§6.7 and sets `status = "fixed"`, `fixed_by` (D§12). Refused for a promoted record (D§6.6). |
| Skip | — | nothing | The record stays `open`. |

**(operator)** A record never becomes a rule without this review. Only `ratify --promote rule` writes `promoted_to = "rule"`, and D§6.6 forbids agents to write it. The node that writes the rule cites the record and its `reviewed` date, which the rule clause of the skills accepts as the operator's approval (execute-work `SKILL.md:8-15`).

**(author)** Before each write the verb reads the file again. When its bytes differ from what the verb showed, it writes nothing and prints `<id> changed since it was shown; skipped`. This covers two reviews at once and an edit during a review. The write is a temporary file in `docs/decisions/` and a rename, so a crash leaves the old file or the new one. The verb holds an exclusive lock on `<git common dir>/zygo-decisions.lock` from before the re-read until after the rename, so of two writers of one record only one writes; the other waits, reads the renamed file and skips it. The lock file is never written, renamed or removed, and it is not the record: the rename replaces the record, and on Windows a lock on the record blocks reads of it. `ratify` and `overturn` therefore need write access to the git common dir, the same as `zygo-work` in the git common dir; without it they print `zygo decision: cannot write <lock path>: <error>` and change nothing.

## 12. Changes outside this repository

**(operator)** The plan, execute and deliver skills are in the act101 plugin; the act-lite plugin carries the same skills. P3-8 names the changes; the plugin makes them. **(author)** The changes, each against the act-lite text. **(lead)** Items 3 to 12 ship before a zygo release that has the `dispositions` guard (D§9.3), because that guard refuses today's `keep` call.

**plan-work** (`plan-work/SKILL.md:126-176`, header in `work-graph/references/graph-format.md:36-57`):

1. The graph header gets a `**Delegation:**` line: what an author decides alone, what it records, and the stop classes (D§8.3, D§8.2), citing this spec.
2. When `zygo decision ls --pending` lists records, the plan has one node per record: reverse an `overturned` record (its commits, D§6.7) and set it `fixed`; or write a promotion and set `promoted_by`. Each commit carries `Zygo-Decision: <id>`.

**execute-work** (`execute-work/SKILL.md:140-164` verification stage, `:215-230` keep or discard, `:240-279` handoff, `:315-326` merge):

3. The reviewer reports, for each finding, a label and a class (D§8.2), with the location, failure and evidence. In round 3 it also states, for each finding, whether it accepts a record if the finding is not fixed (D§8.4).
4. Rounds 1 and 2: a confirmed finding is fixed, or the round fails. Round 3: after the review, the implementer runs the records step of D§9.7, with `docs/decisions/` in the handoff's `Files:` line. A refuted finding of class `none` is rejected with its evidence; a refuted stop-class finding is recorded with kind `decision` (D§8.2).
5. During the node's work the implementer records each assumption and decision of D§8.3, under the node's unit.
6. Each record commit carries `Zygo-Decision: <id>`.
7. The node file gets a `## Review` section per round: each finding label, class, disposition and its proof.
8. Call `work_verdict` with `keep`, `head` and `dispositions` (an empty list when there were no findings) **before** the merge of §5.5 step 1, after the verification stage. In one checkout, also give `base`. A refused keep with only stop problems: set the node `BLOCKED(stop: <class> <ids>)`, keep its branch and checkout, and ask the operator with the record ids and the checkout path (D§8.5); after the operator's answer, call `keep` again. Any other refused keep is a failed stage. Today the skill calls `work_verdict` after the keep or discard (`SKILL.md:224-230`).
9. After a recorded `keep`, when the merge of §5.5 step 1 conflicts, or the check of §5.5 step 2 fails, abandon the merge and call `work_verdict` with `rework` for the same unit and round. The verdict word on the row is replaced (`crates/zygo-usage/src/store.rs:292-295`). §5.5 step 2 (`SKILL.md:319-322`) already returns the node to a new round on a failed check; a conflicted merge is not covered there today and is treated the same way.
10. Two current defects in the same step, found in this design's recon. The skill sends `verdict = "blocked"`, which zygo refuses (`crates/zygo-mcp/src/tools/work.rs:16`, `:68-72`): send `discard` for a `BLOCKED` node that is not a stop. The skill sends `findings` as a number of findings, and zygo refuses a value that is not a string (`work.rs:73-75`): send it as a string.

**deliver-work** (`deliver-work/SKILL.md:94-97`, pull request body):

11. Run `zygo decision check` with the step 2 checks.
12. The body lists each record added or changed on the branch (`git diff --name-status <base>...HEAD -- docs/decisions/`): id, kind, status, class, title. It lists each fixed stop-class finding with its commit (D§8.2), taken from the node files' `## Review` sections. It gives the `decision records:` line of `zygo work report` (D§9.5).

## 13. Base-spec changes

**(author)** When this spec is approved, the base spec changes as below. Each change is written into the section as final text, with a change-log line.

1. §13.3 `work_verdict`: the `head`, `base` and `dispositions` input (D§9.2) and the check (D§9.4).
2. §13.4: one row for the `dispositions` guard (D§9.3), and its default `enforce` in the defaults sentence ("Defaults: all `monitor` except `trailers = "notify"`"). The sentence "All guards except `lead`, `redaction` and `attribution` run only when `[features] work_units` is on" holds for it.
3. §13.5: `dispositions` is a valid key of `[guards]`.
4. §13.6: the two report lines (D§9.5).
5. §17 and §17.1: the `zygo decision` verb and its slash form (D§10.1).
6. The session view (`crates/zygo-core/src/session_view.rs:7-42`) gains `repo_root: Option<String>` with `#[serde(default)]`: the repository top level that the launcher already holds (`crates/zygo-launch/src/launcher.rs:633`). The `work_verdict` tool runs git there. A view without it (an older launcher) gives the problem `cannot check: no repository root`.

§14 does not change: no column, no table.

## 14. Failure handling

**(author)** No hook reads a record (D-C2). The entry points that read records, and their behavior:

| Case | `work_verdict` (D§9.4) | `zygo work report` | `ls`, `show`, `deps` | `check` | `review`, `ratify`, `overturn` |
|---|---|---|---|---|---|
| `docs/decisions/` absent | a named `record` is missing: a problem; nothing else | no records lines | empty list, exit 0 | exit 0 | `no open records`, exit 0 |
| `docs/decisions` a link or not a directory | in git: its paths are not record blobs, so a named record is missing | its D§6.8 line | exit 1 | exit 1 | exit 1 |
| `docs/decisions` cannot be read (`lstat` error other than absent, listing or entry error) | not affected: it reads records from git (D§9.1) | its D§6.8 line | exit 1 | exit 1 | exit 1 |
| A malformed file | a problem only when the node names it or changes it | counted in `malformed` | one stderr line per file, skipped | one line per fault, exit 1 | one stderr line; it cannot be reviewed or changed |
| A file over 64 KiB, a link, a FIFO | as malformed, never opened | as malformed | as malformed | as malformed | as malformed |
| A git process fails or passes its bound | `cannot check: …` (refused at `enforce`) | no records lines | the git-backed part (commits in `show`) is left out with one stderr line | as for `ls` | as for `show` |
| The write of an outcome fails | — | — | — | — | exit 1 naming the file; earlier outcomes are kept |
| Two writers of one record | — | — | — | — | D§11.3 re-read: skipped |

Two parallel nodes: each writes new files with new ULIDs, so their merges do not conflict. Two nodes that change the same record conflict at the merge of §5.5 step 1 (execute-work `SKILL.md:317-318`); the skill abandons that merge and sends `rework` (D§12 item 9).

## 15. Owners of state

| State | Owner | Readers |
|---|---|---|
| A record's fields, body and status | its file in git | all `zygo decision` subcommands, `work_verdict`, `zygo work report` |
| Dependents, counts, depth | none: computed at each read | as above |
| The node range | none: computed per verdict from `base` (or `HEAD`) and `head` | `work_verdict` |
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
- **A default of `monitor` (an opt-in gate)**: the P3-9 row says the verdict step refuses (D§9.3) **(lead)**.
- **A count of disposition events in `guard_events`**: the fold rewrites that column (D§9.3).
- **A guard on `Zygo-Decision:`**: D§9.6.
- **The operator ratifies a stopped node's record on the work branch**: an add/add conflict on the record file at each later merge (D§8.5).
- **An approval file outside the tree for a stopped node** (as `zygo-work` in the git common dir, §13.2): a second owner of the operator's outcome, which the record file already holds.
- **Any older commit as the proof of `fixed`**: proves nothing about this node (D§8.1).
- **zygo writes the work node of an overturn**: the controller is the only writer of `graph.md` (execute-work `SKILL.md:192-193`), and other work sources have no node format.
- **zygo commits the outcomes of a review**: zygo would change the operator's branch and index; the operator commits.
- **A bulk "overturn all dependents"**: not asked. D§11.2 step 5 shows the dependents next, one by one.
- **`recorded` in any round**: lets an agent skip the review loop (D§8.1).
- **A pattern `docs/decisions/DDR-*.md` in the handoff `Files:` line**: the scope guard does not match patterns (D§9.7).
- **The `--since <ref>` filter on `ls` for the pull request body**: `git diff --name-status` gives the same list (D§12 item 12).

## 17. Acceptance

The verdict tests use a git fixture repository with linked checkouts. The git-failure cases use an injected git runner that returns the failure; no test measures wall-clock time.

1. `zygo decision new` writes a file that `check` reports as malformed until the hint lines are replaced; after that `check` exits 0.
2. Two records created at the same time in two linked checkouts get different ids; merging both branches gives no conflict.
3. A record with a cycle in `depends_on`, a missing `depends_on` id, a file name that differs from `id`, a name `DDR-bad.md`, or a 65 KiB file makes `check` exit 1 with one line each; `ls` exits 0 and names each file on stderr. `README.md`, `x.tmp` and a subdirectory are ignored. A `docs/decisions` that is a link makes `ls` and `check` exit 1.
4. `deps` on an assumption with a chain of 3 records and a fourth record that depends on both the second and the third prints 4 lines with depths 1, 2, 3, 3; the fourth record, reached on two paths, is printed once, at depth 3.
5. With the default level (`enforce`), `work_verdict` keep is refused, and the row's verdict is unchanged, for each of: no `head`; no `dispositions`; a duplicate label; one record for two findings; a `fixed` commit that is an ancestor of `base`; `recorded` in round 2; a recorded record that is absent at `head`, present at `base`, of another unit, or of another class than its entry; a `rejected` entry of class `security`; a recorded `open` record of class `security`; a new `open` record of class `contract` that no finding names; an agent commit that sets a record `ratified`; an agent commit that changes a record's `class`; an agent commit that sets `fixed` with a `fixed_by` outside the node range; an agent commit that deletes a record. Each refusal names its problem.
6. At `enforce`, a keep with one `fixed` (commit in the node range), one `rejected` (class `none`), one `recorded` (round 3, class `none`, open, added in the node range, same unit) and an empty `findings` string is recorded. A keep with `dispositions: []` and no record changes is recorded.
7. Stop path in a node checkout: a keep in round 1 with a recorded `security` record is refused with `stops for the operator` (not with the round limit); the operator runs `zygo decision ratify <id> --yes` in the node checkout without `ZYGO_SESSION_DIR` and commits without a `Zygo-Session:` line; the next keep with the new `head` is recorded. The same with `overturn`: the next keep is refused with `overturned by the operator`; after an agent commit that fixes the subject and sets the record `fixed` with that commit, a keep with the entry `fixed` is recorded. The same flow in one checkout, with `base` given, gives the same results.
8. A keep after the node is merged, without `base`, is refused with `head is already in HEAD`.
9. With the injected git runner returning a timeout for the `git log` step, keep is refused at `enforce` with `cannot check: git log …`, and the tool returns normally.
10. At `notify`, each case of item 5 records the verdict and the reply ends with the problems. At `monitor`, the verdict is recorded, the reply is the current reply, and the injected git runner records no call. The argument refusals of D§9.2 are returned at all three levels.
11. `zygo work report` prints `decisions: 2 open, 3 total` in the unit block and `decision records: 2 open, 3 total, deepest chain 2, 1 malformed` for a fixture repository; outside a repository it prints no records line.
12. `review` refuses to run with `ZYGO_SESSION_DIR` set and without a terminal. It shows records in dependents order, largest first. After an overturn, the open dependents of that record come next. Each outcome is in its file before the next prompt; a second `review` shows only the records still open.
13. A record changed on disk while `review` shows it is skipped, and the file is unchanged. A record title with an ESC character prints as `\u{1b}`.
14. `ratify --promote rule --as <text> --yes` writes `status`, `reviewed`, `promoted_to`, `promotion`, and keeps unknown keys, comments and the body byte-equal. `overturn` of that record exits 1 and changes nothing.
15. `/zygo:decision ls` replies `run in your shell: zygo decision ls` and opens no file.
16. No record text, finding label, reason or disposition is in the usage store, a log or the session directory.

## 18. Premises the first build verifies

- **PD1** `git cat-file --batch-command`, `git merge-base`, `git rev-list` and `git log` run in the main checkout read commits made in a linked checkout of the same repository, before any merge.
- **PD2** Claude Code passes a `work_verdict` argument object of 256 KiB to the MCP server unchanged.
- **PD3** On Claude Code 2.1.328 the `PreToolUse` input for `work_verdict` from the main loop has no `agent_id` key, so the existing lead-only hook lets the signature through (the deny branch at `crates/zygo-work/src/hooks.rs:184-185`).

## Change log

- 2026-11-24 — D§6.2 — author's call — the example commit had 41 hex characters; D§6.3 allows 40 or 64, so it is cut to 40 (found while planning phase3 DR-1).
- 2026-11-24 — D§17 #4 — author's call — with each record printed once at its smallest depth (D§11.2), 3 dependents at depths 1, 2, 3 cannot include one reached on two paths; the case now has a fourth record at depth 3 (found while planning phase3 DR-2).
- 2026-11-24 — D§11.3 — author's call — name the writer lock and its location (Windows mandatory locks, DR-5 review round 2).
- 2026-11-24 — D§9.4 step 8 — author's call — git log lists no paths for a merge commit, so a record added in a merge passed step 8; --diff-merges=first-parent lists them.
- 2026-11-25 — D§6.8 (with D§9.5 and D§14) — author's call (the lead) — an unreadable directory was reported as not a directory; it now has its own case and line, `cannot read docs/decisions` (DR-7 review).
- 2026-11-25 — D§9.4 steps 1, 4, 5 — author's call (DR-13 review) — a short id must resolve to a commit whose id starts with it; a ref of the same name is refused.
- 2026-11-25 — D§9.3, D§9.4 — author's call (DR-13 review) — reading the level runs git for `.zygo.toml` at every level; `monitor` runs no check and none of the four verdict git processes, and the level read is outside the D§9.4 budget.
- 2026-11-25 — D§9.4 step 5 — author's call (DR-13 review) — a `fixed` entry without `commit` gave `commit  is not in the node range` with an empty value; it now gives `<label>: no commit` (D§9.2 does not require `commit`).
- 2026-11-28 — D§9.4 (with D§18 PD1) — author's call (FX-41) — the one `--batch` process started at step 1 and kept that 2 s while `merge-base`, `rev-list` and `log` ran, so the later reads timed out, and it read a whole record before the 64 KiB check; now the three whole processes run first and the process (`--batch-command`) starts after them with its own 2 s (D-C6 unchanged), and the type of `docs/decisions` and the size of a record are asked with `info` before any contents are read.
