export type CompareRow = {
  relativePath: string;
  action: string;
  sourceSize?: number | null;
  destSize?: number | null;
  sourceMtime?: number | null;
  destMtime?: number | null;
};

export type CompareResult = {
  rows: CompareRow[];
  scanned: number;
  toCopy: number;
  unchanged: number;
};

export type SyncStats = {
  scanned: number;
  copied: number;
  skipped: number;
  errors: number;
  bytesCopied: number;
};

export type TaskItem = {
  id: string;
  name: string;
  source: string;
  dest: string;
  lastSyncAt?: string;
  lastStatus?: string;
};

export type FerrisyncApi = {
  pickFolder: () => Promise<string | null>;
  compare: (source: string, destination: string) => Promise<{
    rows: Array<{
      relativePath: string;
      action: string;
      sourceSize?: number | null;
      destSize?: number | null;
      sourceMtime?: number | null;
      destMtime?: number | null;
    }>;
    scanned: number;
    toCopy: number;
    unchanged: number;
  }>;
  sync: (source: string, destination: string) => Promise<{
    scanned: number;
    copied: number;
    skipped: number;
    errors: number;
    bytesCopied: number;
  }>;
  cleanup: (
    source: string,
    destination: string,
    forceEnabled: boolean,
    dryRun: boolean,
  ) => Promise<{
    candidates: number;
    deletedOrQuarantined: number;
    skipped: number;
    dryRunWouldAct: number;
  }>;
  appDataPath: () => Promise<string>;
  listTasks: () => Promise<TaskItem[]>;
  saveTasks: (tasks: TaskItem[]) => Promise<boolean>;
};

declare global {
  interface Window {
    ferrisync: FerrisyncApi;
  }
}

export {};
