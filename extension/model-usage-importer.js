const CHUNK_SIZE = 524288; // 512 KiB

export class UsageImporterWizard {
  constructor({ api, t, onComplete, onError }) {
    this.api = api;
    this.t = t;
    this.onComplete = onComplete;
    this.onError = onError;
    this.sessionId = null;
    this.file = null;
  }

  async processFile(file, onProgress) {
    this.file = file;
    try {
      // 1. Begin Session
      const beginRes = await this.api.beginUsageImport({
        fileName: file.name,
        fileSize: file.size,
      });
      this.sessionId = beginRes.sessionId;

      // 2. Read and Chunk Upload
      const totalBytes = file.size;
      const totalChunks = Math.ceil(totalBytes / CHUNK_SIZE);

      for (let i = 0; i < totalChunks; i++) {
        const start = i * CHUNK_SIZE;
        const end = Math.min(start + CHUNK_SIZE, totalBytes);
        const chunkBlob = file.slice(start, end);
        const arrayBuffer = await chunkBlob.arrayBuffer();
        const base64 = arrayBufferToBase64(arrayBuffer);

        await this.api.chunkUsageImport({
          sessionId: this.sessionId,
          chunkIndex: i,
          chunkDataBase64: base64,
        });

        if (onProgress) {
          onProgress({
            currentChunk: i + 1,
            totalChunks,
            percent: Math.round(((i + 1) / totalChunks) * 100),
          });
        }
      }

      // 3. Preview
      const preview = await this.api.previewUsageImport({
        sessionId: this.sessionId,
      });

      return preview;
    } catch (err) {
      if (this.sessionId) {
        this.api.cancelUsageImport({ sessionId: this.sessionId }).catch(() => {});
      }
      throw err;
    }
  }

  async commit() {
    if (!this.sessionId) throw new Error('No active import session');
    const result = await this.api.commitUsageImport({ sessionId: this.sessionId });
    this.sessionId = null;
    return result;
  }

  cancel() {
    if (this.sessionId) {
      this.api.cancelUsageImport({ sessionId: this.sessionId }).catch(() => {});
      this.sessionId = null;
    }
  }
}

function arrayBufferToBase64(buffer) {
  let binary = '';
  const bytes = new Uint8Array(buffer);
  const len = bytes.byteLength;
  for (let i = 0; i < len; i++) {
    binary += String.fromCharCode(bytes[i]);
  }
  return btoa(binary);
}
