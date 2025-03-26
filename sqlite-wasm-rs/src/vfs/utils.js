export class JSUtils {
  static toSlice(mem, src, dst, len) {
    const slice = new Uint8Array(mem.buffer, dst, len);
    slice.set(src, 0);
  }

  static toUint8Array(mem, src, len, dst) {
    const slice = new Uint8Array(mem.buffer, src, len);
    dst.set(slice, 0);
  }
}
