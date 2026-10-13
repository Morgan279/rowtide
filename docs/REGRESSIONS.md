# Public regression guidance

Use small constructed inputs to capture repeatable mechanisms. Keep fixed
expected values independent from the implementation being tested.

| Mechanism | Public regression |
|---|---|
| A subset is mistaken for a full population | Explicit expected-key coverage with row/distinct-key denominators |
| Same schema, different content or purpose | Content, role, projection, condition and contract changes invalidate reuse |
| Reader bytes or parameters change | Reader identity participates in reuse and native comparison |
| Scope changes meaning | Explicit filters and the complete expected population remain in check identity |
| A historical statement is treated as current evidence | Source digest, exact pointer/excerpt, input binding and origin are required |
| Native evidence has only a path | Unknown identity is retained; complete declared content has a separate positive case |
| Measurement is interrupted | Unknown/partial replaces incomplete statistics, without a false supported result |
| A task resumes from another directory | Normalized saved requests still rehash current dependencies |
| Record is mistaken for a new measurement | Zero current check counters, carried findings and historical reason labels |
| A consumption difference lacks guidance | JSON and summary include actionable mismatched/unknown fields |
| Literal names inject Markdown structure | Real Parquet fields and requested types exercise delimiters and controls |
| Saved metadata is mistaken for validated intent | Unmapped analysis changes can keep input matches; the consumer owns expectations |

For a new case, document the trigger, intended behavior, unsupported boundary
and minimal synthetic inputs. Do not relabel copied operational data as a
fictional fixture. A parser or business algorithm not executed by Rowtide must
not be described as independently verified. Run public tests with
`cargo test --locked`; runtime outputs belong in fresh directories outside the
source snapshot. Completion and supported findings are separate dimensions.
