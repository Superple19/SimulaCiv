# Superseded M4-02 incremental diagnostic

These files are retained as the first same-session attempt. Code inspection found that the direct path used incremental topology while `BranchRequest.execution` remained `Serial`, so this was not an apples-to-apples gate measurement and must not be used for the final verdict.

| File | Contents |
|---|---|
| [`paired_samples.tsv`](paired_samples.tsv) | Raw three-revision pairs from the invalid mode comparison. |
| [`run.log`](run.log) | Timing markers, host samples, and the invalid-mode summaries. |
