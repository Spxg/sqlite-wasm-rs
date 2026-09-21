// Test-only faults; Rust guards restore the browser methods after each case.
export function failNext(method, count, name) {
    const prototype = FileSystemSyncAccessHandle.prototype;
    const original = prototype[method];
    prototype[method] = function (...args) {
        if (count > 0) {
            count--;
            throw new DOMException("injected OPFS failure", name);
        }
        return original.apply(this, args);
    };
    return () => { prototype[method] = original; };
}

export class AcquisitionGate {
    constructor(skip) {
        const prototype = FileSystemFileHandle.prototype;
        const original = prototype.createSyncAccessHandle;
        let acquired;
        let closed;
        let releaseHandle;
        let released = false;

        this.acquired = new Promise(resolve => { acquired = resolve; });
        this.closed = new Promise(resolve => { closed = resolve; });
        this.release = () => {
            released = true;
            if (releaseHandle) releaseHandle();
        };
        this.restore = () => {
            prototype.createSyncAccessHandle = original;
            this.release();
        };

        prototype.createSyncAccessHandle = function (...args) {
            if (skip-- > 0) return original.apply(this, args);
            prototype.createSyncAccessHandle = original;

            return original.apply(this, args).then(handle => {
                const close = handle.close;
                handle.close = function () {
                    close.call(this);
                    closed();
                };

                // Acquire the real browser lock, but delay delivery to Rust.
                return new Promise(resolve => {
                    releaseHandle = () => resolve(handle);
                    acquired();
                    if (released) releaseHandle();
                });
            });
        };
    }
}
