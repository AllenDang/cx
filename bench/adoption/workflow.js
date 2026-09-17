const results = [];
for (let offset = 0; offset < args.records.length; offset += 3) {
  const wave = args.records.slice(offset, offset + 3);
  const completed = await runs.all(wave.map(row => ({
    key: row.id,
    label: "Implement " + row.case,
    agent: "edit-trial",
    model: row.model + ":medium",
    cwd: row.cwd,
    task: row.prompt,
    context: "fresh",
    skill: false,
    timeoutMs: 600000,
    output: row.id + "/result.md",
    acceptance: { level: "none", reason: "Frozen host-side behavioral evaluation, not self-attestation." }
  })));
  for (let i = 0; i < completed.length; i++) {
    const result = completed[i];
    results.push({ id: wave[i].id, ok: result.ok ?? false, runId: result.runId ?? null,
      outputReference: result.outputReference ?? null, outputPathMapping: result.outputPathMapping ?? null,
      artifactPaths: result.artifactPaths ?? [], output: result.output ?? "" });
  }
  emit({ completed: results.length, total: args.records.length, wave: results.slice(-wave.length) });
  if (completed.some(result => !result.ok)) return { blocked: true, results };
}
return { blocked: false, results };
