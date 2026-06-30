import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { getCurrent } from "@tauri-apps/api/window";
import { cancelExportJob, listExportJobs } from "./hooks/useTauri";

const ExportJobsContext = createContext(null);
const ACTIVE = new Set(["queued", "running"]);

export function ExportJobsProvider({ children }) {
  const [jobs, setJobs] = useState([]);
  const jobsRef = useRef(jobs);
  jobsRef.current = jobs;

  const upsert = useCallback((job) => {
    setJobs((current) => [job, ...current.filter((item) => item.id !== job.id)]
      .sort((a, b) => String(b.created_at).localeCompare(String(a.created_at))));
  }, []);

  useEffect(() => {
    listExportJobs().then(setJobs).catch(console.error);
    let stopEvent;
    let stopClose;
    listen("export_job_updated", (event) => upsert(event.payload)).then((stop) => { stopEvent = stop; });
    getCurrent().onCloseRequested((event) => {
      if (jobsRef.current.some((job) => ACTIVE.has(job.status))) {
        event.preventDefault();
        window.alert("Exports are still running. Cancel them or wait for completion before closing.");
      }
    }).then((stop) => { stopClose = stop; });
    return () => { stopEvent?.(); stopClose?.(); };
  }, [upsert]);

  const value = useMemo(() => ({
    jobs,
    activeJobs: jobs.filter((job) => ACTIVE.has(job.status)),
    cancel: cancelExportJob,
    beginLocalJob: (label, kind, dedupeKey) => {
      const existing = jobsRef.current.find((job) => job.dedupe_key === dedupeKey && ACTIVE.has(job.status));
      if (existing) return { job: existing, existing: true };
      const active = jobsRef.current.filter((job) => ACTIVE.has(job.status));
      if (active.length >= 2) return { job: active[0], existing: true, capacityReached: true };
      const job = {
        id: `local-${crypto.randomUUID()}`,
        dedupe_key: dedupeKey,
        kind,
        label,
        status: "running",
        phase: "starting",
        processed_rows: 0,
        total_rows: 0,
        bytes_written: 0,
        percent: 0,
        output_paths: [],
        created_at: new Date().toISOString(),
        cancellable: false,
      };
      jobsRef.current = [job, ...jobsRef.current];
      upsert(job);
      return { job, existing: false };
    },
    updateLocalJob: (id, patch) => {
      setJobs((current) => current.map((job) => job.id === id ? { ...job, ...patch } : job));
    },
    canStartExport: jobs.filter((job) => ACTIVE.has(job.status)).length < 2,
  }), [jobs, upsert]);
  return <ExportJobsContext.Provider value={value}>{children}</ExportJobsContext.Provider>;
}

export const useExportJobs = () => useContext(ExportJobsContext);
