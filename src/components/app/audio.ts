export function encodeWav(samples: Float32Array, sampleRate: number): ArrayBuffer {
  const bytesPerSample = 2;
  const blockAlign = bytesPerSample;
  const buffer = new ArrayBuffer(44 + samples.length * bytesPerSample);
  const view = new DataView(buffer);

  const writeString = (offset: number, value: string) => {
    for (let i = 0; i < value.length; i += 1) {
      view.setUint8(offset + i, value.charCodeAt(i));
    }
  };

  writeString(0, "RIFF");
  view.setUint32(4, 36 + samples.length * bytesPerSample, true);
  writeString(8, "WAVE");
  writeString(12, "fmt ");
  view.setUint32(16, 16, true);
  view.setUint16(20, 1, true);
  view.setUint16(22, 1, true);
  view.setUint32(24, sampleRate, true);
  view.setUint32(28, sampleRate * blockAlign, true);
  view.setUint16(32, blockAlign, true);
  view.setUint16(34, 16, true);
  writeString(36, "data");
  view.setUint32(40, samples.length * bytesPerSample, true);

  let offset = 44;
  for (let i = 0; i < samples.length; i += 1) {
    const s = Math.max(-1, Math.min(1, samples[i]));
    view.setInt16(offset, s < 0 ? s * 0x8000 : s * 0x7fff, true);
    offset += 2;
  }

  return buffer;
}

export async function blobToMono16kWav(blob: Blob): Promise<ArrayBuffer> {
  const srcCtx = new AudioContext();
  const arrayBuffer = await blob.arrayBuffer();
  const source = await srcCtx.decodeAudioData(arrayBuffer.slice(0));

  const targetRate = 16000;
  const offline = new OfflineAudioContext(1, Math.ceil(source.duration * targetRate), targetRate);
  const src = offline.createBufferSource();

  const mono = offline.createBuffer(1, source.length, source.sampleRate);
  const merged = mono.getChannelData(0);
  for (let ch = 0; ch < source.numberOfChannels; ch += 1) {
    const data = source.getChannelData(ch);
    for (let i = 0; i < data.length; i += 1) {
      merged[i] += data[i] / source.numberOfChannels;
    }
  }

  src.buffer = mono;
  src.connect(offline.destination);
  src.start(0);

  const rendered = await offline.startRendering();
  await srcCtx.close();

  return encodeWav(rendered.getChannelData(0), targetRate);
}
