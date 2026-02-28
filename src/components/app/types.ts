export type RecordingItem = {
  id: string;
  name: string;
  filePath: string;
  createdAtMs: number;
};

export type SaveAndTranscribeResult = {
  recording: RecordingItem;
  text: string;
};

export type ModelInitStatus = {
  running: boolean;
  phase: string;
  progress: number;
  message: string;
  ready: boolean;
  error?: string | null;
};
