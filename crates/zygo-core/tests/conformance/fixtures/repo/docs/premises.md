# Fixture premises

Alpha: 1.0.0
Beta: 2.0.0

## Results

| Id | Item | Agent | Version | Date | Result | Evidence |
|---|---|---|---|---|---|---|
| Q1 | An item with a \| in its text | Alpha | 1.0.0 | 2026-11-25 | verified | `fixtures/alpha/a.json` |
| Q2 | A failed item | Alpha | 1.0.0 | 2026-11-25 | failed | `fixtures/alpha/a.json` |
| Q3 | An observed item | Alpha | 1.0.0 | 2026-11-25 | observed | `fixtures/alpha/a.json` |
| Q9 | A row given twice | Alpha | 1.0.0 | 2026-11-25 | verified | `fixtures/alpha/a.json` |
| Q9 | A row given twice | Alpha | 1.0.0 | 2026-11-25 | verified | `fixtures/alpha/a.json` |
| Q4 | A joint item | Alpha + Beta | 1.0.0 | 2026-11-25 | verified | `fixtures/alpha/a.json` |
