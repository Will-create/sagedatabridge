import { useState } from "react";
import { useExportJobs } from "../exportJobs";

export default function ExportProgressCenter() {
  const { jobs, activeJobs, cancel } = useExportJobs();
  const [open, setOpen] = useState(false);
  if (!jobs.length) return null;
  return (
    <div className="export-center">
      <button type="button" className="export-center-trigger" onClick={() => setOpen((value) => !value)}>
        Exports {activeJobs.length ? `(${activeJobs.length})` : ""}
      </button>
      {open ? <div className="export-center-panel">{jobs.slice(0, 8).map((job) => (
        <div className="export-job" key={job.id}>
          <div className="export-job-head"><strong>{job.label}</strong><span>{job.percent}%</span></div>
          <div className="export-job-track"><span style={{ width: `${job.percent}%` }} /></div>
          <div className="export-job-meta">
            <span>{job.phase} · {Number(job.processed_rows || 0).toLocaleString()} / {Number(job.total_rows || 0).toLocaleString()}</span>
            {job.cancellable ? <button type="button" onClick={() => cancel(job.id)}>Cancel</button> : null}
          </div>
          {job.error ? <div className="export-job-error">{job.error}</div> : null}
        </div>
      ))}</div> : null}
    </div>
  );
}
