import { useState, useEffect, useCallback } from "react";
import {
  listCronJobs,
  getCronRuns,
  createCronJob,
  updateCronJob,
  deleteCronJob,
} from "../api";
import type { CronJob, CronRun } from "../api";
import { clipCorner } from "../theme";

interface Props {
  instanceId: string;
  toast: (msg: string, isError?: boolean) => void;
}

export default function Cron({ instanceId, toast }: Props) {
  const [jobs, setJobs] = useState<CronJob[]>([]);
  const [selected, setSelected] = useState<CronJob | null>(null);
  const [runs, setRuns] = useState<CronRun[]>([]);
  const [loading, setLoading] = useState(false);
  const [showCreate, setShowCreate] = useState(false);

  // Create form
  const [formName, setFormName] = useState("");
  const [formExpression, setFormExpression] = useState("");
  const [formJobType, setFormJobType] = useState("agent");
  const [formCommand, setFormCommand] = useState("");
  const [formPrompt, setFormPrompt] = useState("");

  const fetchJobs = useCallback(() => {
    setLoading(true);
    listCronJobs(instanceId)
      .then((res) => {
        setJobs(res.jobs);
      })
      .catch((err) => toast(err.message, true))
      .finally(() => setLoading(false));
  }, [instanceId, toast]);

  useEffect(() => {
    fetchJobs();
  }, [fetchJobs]);

  const selectJob = useCallback(
    (job: CronJob | null) => {
      setSelected(job);
      if (job) {
        getCronRuns(instanceId, job.id)
          .then((res) => setRuns(res.runs))
          .catch(() => setRuns([]));
      } else {
        setRuns([]);
      }
    },
    [instanceId],
  );

  const handleCreate = useCallback(async () => {
    if (!formName.trim() || !formExpression.trim()) {
      toast("Name and expression are required", true);
      return;
    }
    try {
      await createCronJob(instanceId, {
        name: formName.trim(),
        expression: formExpression.trim(),
        job_type: formJobType,
        command: formJobType === "shell" ? formCommand.trim() : undefined,
        prompt: formJobType === "agent" ? formPrompt.trim() : undefined,
      });
      toast("Cron job created");
      setShowCreate(false);
      setFormName("");
      setFormExpression("");
      setFormCommand("");
      setFormPrompt("");
      fetchJobs();
    } catch (err: unknown) {
      toast(err instanceof Error ? err.message : "Create failed", true);
    }
  }, [instanceId, formName, formExpression, formJobType, formCommand, formPrompt, fetchJobs, toast]);

  const handleToggle = useCallback(
    async (job: CronJob) => {
      try {
        await updateCronJob(instanceId, job.id, { enabled: !job.enabled });
        toast(job.enabled ? "Job disabled" : "Job enabled");
        fetchJobs();
      } catch (err: unknown) {
        toast(err instanceof Error ? err.message : "Update failed", true);
      }
    },
    [instanceId, fetchJobs, toast],
  );

  const handleDelete = useCallback(
    async (jobId: string) => {
      try {
        await deleteCronJob(instanceId, jobId);
        toast("Job deleted");
        setSelected(null);
        setRuns([]);
        fetchJobs();
      } catch (err: unknown) {
        toast(err instanceof Error ? err.message : "Delete failed", true);
      }
    },
    [instanceId, fetchJobs, toast],
  );

  const inputStyle: React.CSSProperties = {
    fontFamily: "JetBrains Mono, monospace",
    fontSize: 13,
    background: "var(--bg-input)",
    border: "1px solid var(--border)",
    color: "var(--text-primary)",
    padding: "6px 10px",
    clipPath: clipCorner(6),
    outline: "none",
    flex: 1,
  };

  const btnSecondary: React.CSSProperties = {
    fontFamily: "JetBrains Mono, monospace",
    fontSize: 11,
    fontWeight: 600,
    textTransform: "uppercase",
    letterSpacing: 1,
    padding: "7px 16px",
    background: "transparent",
    border: "1px solid var(--border)",
    color: "var(--text-primary)",
    cursor: "pointer",
    clipPath: clipCorner(6),
  };

  const btnPrimary: React.CSSProperties = {
    ...btnSecondary,
    background: "var(--amber)",
    border: "1px solid var(--amber)",
    color: "#000",
  };

  return (
    <div style={{ padding: 24, maxWidth: 1000, flex: 1, overflowY: "auto" }}>
      <div style={{ display: "flex", alignItems: "baseline", gap: 12, marginBottom: 20 }}>
        <h2
          style={{
            fontFamily: "Syne, sans-serif",
            fontSize: 18,
            fontWeight: 700,
            color: "var(--amber)",
            textTransform: "uppercase",
            letterSpacing: 1,
          }}
        >
          Cron Jobs
        </h2>
        <span
          style={{
            fontFamily: "JetBrains Mono, monospace",
            fontSize: 11,
            color: "var(--text-dim)",
          }}
        >
          {jobs.length} {jobs.length === 1 ? "job" : "jobs"}
        </span>
      </div>

      {/* Toolbar */}
      <div style={{ display: "flex", gap: 8, marginBottom: 16 }}>
        <button onClick={fetchJobs} style={btnSecondary}>
          Refresh
        </button>
        <button
          onClick={() => setShowCreate(!showCreate)}
          style={btnPrimary}
        >
          + New Job
        </button>
      </div>

      {/* Create form */}
      {showCreate && (
        <div
          style={{
            background: "var(--bg-card)",
            border: "1px solid var(--border)",
            clipPath: clipCorner(10),
            padding: 16,
            marginBottom: 16,
          }}
        >
          <div style={{ display: "flex", gap: 8, marginBottom: 8 }}>
            <input
              type="text"
              placeholder="Job name"
              value={formName}
              onChange={(e) => setFormName(e.target.value)}
              style={inputStyle}
            />
            <input
              type="text"
              placeholder="Cron expression (e.g. 0 */5 * * * *)"
              value={formExpression}
              onChange={(e) => setFormExpression(e.target.value)}
              style={{ ...inputStyle, flex: 2 }}
            />
          </div>
          <div style={{ display: "flex", gap: 8, marginBottom: 8 }}>
            <select
              value={formJobType}
              onChange={(e) => setFormJobType(e.target.value)}
              style={{ ...inputStyle, flex: "unset", width: 140 }}
            >
              <option value="agent">Agent</option>
              <option value="shell">Shell</option>
            </select>
            {formJobType === "shell" ? (
              <input
                type="text"
                placeholder="Shell command"
                value={formCommand}
                onChange={(e) => setFormCommand(e.target.value)}
                style={inputStyle}
              />
            ) : (
              <input
                type="text"
                placeholder="Agent prompt"
                value={formPrompt}
                onChange={(e) => setFormPrompt(e.target.value)}
                style={inputStyle}
              />
            )}
          </div>
          <div style={{ display: "flex", gap: 8, justifyContent: "flex-end" }}>
            <button
              onClick={() => {
                setShowCreate(false);
                setFormName("");
                setFormExpression("");
                setFormCommand("");
                setFormPrompt("");
              }}
              style={btnSecondary}
            >
              Cancel
            </button>
            <button onClick={handleCreate} style={btnPrimary}>
              Create
            </button>
          </div>
        </div>
      )}

      {loading && (
        <div
          style={{
            color: "var(--text-dim)",
            fontFamily: "Outfit, sans-serif",
            fontSize: 14,
            padding: 16,
          }}
        >
          Loading...
        </div>
      )}

      {/* Jobs list + detail */}
      <div style={{ display: "flex", gap: 16 }}>
        {/* List */}
        <div
          style={{
            flex: 1,
            background: "var(--bg-card)",
            border: "1px solid var(--border)",
            clipPath: clipCorner(10),
            overflow: "hidden",
          }}
        >
          {jobs.length === 0 && !loading && (
            <div
              style={{
                padding: 32,
                textAlign: "center",
                color: "var(--text-dim)",
                fontFamily: "Outfit, sans-serif",
                fontSize: 13,
              }}
            >
              <div
                style={{
                  fontSize: 28,
                  fontFamily: "JetBrains Mono, monospace",
                  marginBottom: 8,
                  opacity: 0.4,
                }}
              >
                {"\u23F0"}
              </div>
              No cron jobs configured
              <div style={{ fontSize: 11, marginTop: 4, color: "var(--text-dim)" }}>
                Use "+ New Job" to schedule tasks
              </div>
            </div>
          )}
          {jobs.map((job) => {
            const isSelected = selected?.id === job.id;
            return (
              <div
                key={job.id}
                onClick={() => selectJob(isSelected ? null : job)}
                style={{
                  padding: "10px 14px",
                  borderBottom: "1px solid var(--border)",
                  cursor: "pointer",
                  borderLeft: isSelected
                    ? "3px solid var(--amber)"
                    : "3px solid transparent",
                  background: isSelected ? "var(--amber-glow)" : "transparent",
                  transition: "background 0.15s",
                }}
              >
                <div
                  style={{
                    display: "flex",
                    alignItems: "center",
                    gap: 8,
                    marginBottom: 2,
                  }}
                >
                  <span
                    style={{
                      width: 8,
                      height: 8,
                      borderRadius: "50%",
                      background: job.enabled ? "var(--success)" : "var(--toggle-off)",
                      flexShrink: 0,
                    }}
                  />
                  <span
                    style={{
                      fontFamily: "JetBrains Mono, monospace",
                      fontSize: 12,
                      fontWeight: 600,
                      color: "var(--amber-bright)",
                    }}
                  >
                    {job.name || job.id.slice(0, 8)}
                  </span>
                  <span
                    style={{
                      marginLeft: "auto",
                      fontFamily: "JetBrains Mono, monospace",
                      fontSize: 10,
                      color: "var(--text-dim)",
                      textTransform: "uppercase",
                    }}
                  >
                    {job.job_type}
                  </span>
                </div>
                <div
                  style={{
                    fontFamily: "JetBrains Mono, monospace",
                    fontSize: 10,
                    color: "var(--text-dim)",
                    marginBottom: 2,
                    paddingLeft: 16,
                  }}
                >
                  {job.expression}
                </div>
                {job.last_status && (
                  <div
                    style={{
                      fontFamily: "Outfit, sans-serif",
                      fontSize: 11,
                      color:
                        job.last_status === "success"
                          ? "var(--success)"
                          : job.last_status === "error"
                            ? "var(--error-text)"
                            : "var(--text-dim)",
                      paddingLeft: 16,
                    }}
                  >
                    Last: {job.last_status}
                    {job.last_run ? ` @ ${job.last_run.replace("T", " ").slice(0, 19)}` : ""}
                  </div>
                )}
              </div>
            );
          })}
        </div>

        {/* Detail panel */}
        {selected && (
          <div
            style={{
              width: 380,
              flexShrink: 0,
              border: "1px solid var(--border-amber)",
              clipPath: clipCorner(10),
              padding: 16,
              background: "var(--bg-card)",
              overflowY: "auto",
              maxHeight: "calc(100vh - 200px)",
            }}
          >
            <div
              style={{
                display: "flex",
                alignItems: "center",
                justifyContent: "space-between",
                marginBottom: 12,
              }}
            >
              <span
                style={{
                  fontFamily: "JetBrains Mono, monospace",
                  fontSize: 14,
                  fontWeight: 700,
                  color: "var(--amber-bright)",
                }}
              >
                {selected.name || selected.id.slice(0, 8)}
              </span>
              <div style={{ display: "flex", gap: 6 }}>
                <button
                  onClick={() => handleToggle(selected)}
                  style={{
                    ...btnSecondary,
                    padding: "4px 10px",
                    fontSize: 10,
                  }}
                >
                  {selected.enabled ? "Disable" : "Enable"}
                </button>
                <button
                  onClick={() => handleDelete(selected.id)}
                  style={{
                    ...btnSecondary,
                    padding: "4px 10px",
                    fontSize: 10,
                    borderColor: "var(--error-text)",
                    color: "var(--error-text)",
                  }}
                >
                  Delete
                </button>
              </div>
            </div>

            {/* Job details */}
            <div
              style={{
                fontFamily: "Outfit, sans-serif",
                fontSize: 12,
                color: "var(--text-dim)",
                display: "flex",
                flexDirection: "column",
                gap: 6,
                marginBottom: 16,
              }}
            >
              <DetailRow label="ID" value={selected.id} />
              <DetailRow label="Expression" value={selected.expression} />
              <DetailRow label="Type" value={selected.job_type} />
              <DetailRow label="Enabled" value={selected.enabled ? "Yes" : "No"} />
              {selected.command && <DetailRow label="Command" value={selected.command} />}
              {selected.prompt && <DetailRow label="Prompt" value={selected.prompt} />}
              {selected.next_run && <DetailRow label="Next Run" value={selected.next_run} />}
              {selected.last_run && <DetailRow label="Last Run" value={selected.last_run} />}
              {selected.last_status && <DetailRow label="Last Status" value={selected.last_status} />}
              <DetailRow label="Created" value={selected.created_at} />
            </div>

            {/* Last output */}
            {selected.last_output && (
              <div style={{ marginBottom: 16 }}>
                <div
                  style={{
                    fontFamily: "JetBrains Mono, monospace",
                    fontSize: 10,
                    color: "var(--text-dim)",
                    textTransform: "uppercase",
                    letterSpacing: 1,
                    marginBottom: 4,
                  }}
                >
                  Last Output
                </div>
                <div
                  style={{
                    background: "var(--bg-input)",
                    fontFamily: "JetBrains Mono, monospace",
                    fontSize: 11,
                    color: "var(--text-primary)",
                    padding: 10,
                    whiteSpace: "pre-wrap",
                    wordBreak: "break-word",
                    maxHeight: 150,
                    overflowY: "auto",
                    clipPath: clipCorner(6),
                  }}
                >
                  {selected.last_output}
                </div>
              </div>
            )}

            {/* Recent runs */}
            <div>
              <div
                style={{
                  fontFamily: "JetBrains Mono, monospace",
                  fontSize: 10,
                  color: "var(--text-dim)",
                  textTransform: "uppercase",
                  letterSpacing: 1,
                  marginBottom: 6,
                }}
              >
                Recent Runs ({runs.length})
              </div>
              {runs.length === 0 && (
                <div
                  style={{
                    fontFamily: "Outfit, sans-serif",
                    fontSize: 11,
                    color: "var(--text-dim)",
                    padding: "8px 0",
                  }}
                >
                  No runs recorded
                </div>
              )}
              {runs.map((run) => (
                <div
                  key={run.id}
                  style={{
                    padding: "6px 8px",
                    borderBottom: "1px solid var(--border)",
                    fontSize: 11,
                    fontFamily: "JetBrains Mono, monospace",
                  }}
                >
                  <div style={{ display: "flex", justifyContent: "space-between" }}>
                    <span
                      style={{
                        color:
                          run.status === "success"
                            ? "var(--success)"
                            : run.status === "error"
                              ? "var(--error-text)"
                              : "var(--text-dim)",
                      }}
                    >
                      {run.status}
                    </span>
                    <span style={{ color: "var(--text-dim)" }}>
                      {run.duration_ms}ms
                    </span>
                  </div>
                  <div style={{ color: "var(--text-dim)", fontSize: 10 }}>
                    {run.started_at.replace("T", " ").slice(0, 19)}
                  </div>
                  {run.output && (
                    <div
                      style={{
                        color: "var(--text-primary)",
                        fontSize: 10,
                        marginTop: 2,
                        whiteSpace: "pre-wrap",
                        maxHeight: 60,
                        overflow: "hidden",
                      }}
                    >
                      {run.output.slice(0, 200)}
                    </div>
                  )}
                </div>
              ))}
            </div>
          </div>
        )}
      </div>
    </div>
  );
}

function DetailRow({ label, value }: { label: string; value: string }) {
  return (
    <span>
      {label}:{" "}
      <span style={{ color: "var(--text-primary)" }}>{value}</span>
    </span>
  );
}
