const MAX_FRAME = 65536;
const protocol = "hyve.room.v1";
const closed = () => new Error("room connection closed");

export function roomSleep(milliseconds) {
    return new Promise(resolve => setTimeout(resolve, milliseconds));
}

// Owns the browser handles from creation so dropping a view can cancel a
// pending handshake, a stream read or a connected WebSocket.
export class BrowserRoom {
    constructor(url, quic, pin, timeout) {
        this.stopped = false;
        this.buffer = new Uint8Array();
        this.messages = [];
        this.reading = new Set();
        this.timer = setTimeout(() => this.close(), timeout);
        try {
            if (quic) {
                const options = pin.length ? { serverCertificateHashes: [{ algorithm: "sha-256", value: pin.slice() }] } : {};
                this.wt = new WebTransport(url, options);
                this.wt.closed.then(() => this.close(), () => this.close());
            } else {
                const ws = this.ws = new WebSocket(url, protocol);
                ws.binaryType = "arraybuffer";
                this.opened = new Promise((resolve, reject) => {
                    this.rejectOpen = reject;
                    ws.onopen = () => ws.protocol === protocol ? resolve() : this.close();
                });
                ws.onclose = ws.onerror = () => this.close();
                ws.onmessage = ({ data }) => {
                    if (!(data instanceof ArrayBuffer) || data.byteLength > MAX_FRAME || this.messages.length >= 128) {
                        this.close();
                        return;
                    }
                    const bytes = new Uint8Array(data);
                    if (this.waiter) {
                        const { resolve } = this.waiter;
                        this.waiter = undefined;
                        resolve(bytes);
                    } else {
                        this.messages.push({bytes});
                    }
                };
            }
        } catch (error) {
            this.close();
            throw error;
        }
    }

    async ready() {
        if (this.wt) {
            await this.wt.ready;
            const stream = await this.wt.createBidirectionalStream();
            this.writer = stream.writable.getWriter();
            this.reader = stream.readable.getReader();
            await this.writer.write(Uint8Array.of(72, 89, 86, 49));
        } else {
            await this.opened;
        }
        const hello = await this.read();
        clearTimeout(this.timer);
        return hello;
    }

    async exact(length) {
        const bytes = new Uint8Array(length);
        let offset = 0;
        while (offset < length) {
            if (!this.buffer.length) {
                const { done, value } = await this.reader.read();
                if (done) throw closed();
                this.buffer = value;
            }
            const count = Math.min(length - offset, this.buffer.length);
            bytes.set(this.buffer.subarray(0, count), offset);
            this.buffer = this.buffer.subarray(count);
            offset += count;
        }
        return bytes;
    }

    extensions(hints, single) {
        this.single = single;
        if (this.wt && hints) {
            this.hintsIn = this.wt.datagrams.readable.getReader();
            this.hintsOut = this.wt.datagrams.writable.getWriter();
        }
    }

    async nextFrame() {
        const header = await this.exact(4);
        const length = new DataView(header.buffer).getUint32(0);
        if (length > MAX_FRAME) throw closed();
        return this.exact(length);
    }

    async pump(lane) {
        if (this.reading.has(lane)) return;
        this.reading.add(lane);
        try {
            let bytes;
            if (lane === "control") bytes = await this.nextFrame();
            else {
                const {value, done} = await this.hintsIn.read();
                if (done || value.length > 1024) throw closed();
                bytes = value;
            }
            if (this.stopped) return;
            if (this.waiter) {
                const {resolve} = this.waiter;
                this.waiter = undefined;
                this.reading.delete(lane);
                resolve(bytes);
            } else this.messages.push({lane, bytes});
        } catch (_) { this.close(); }
    }

    async read() {
        if (this.stopped) throw closed();
        if (this.wt) {
            this.pump("control");
            if (this.hintsIn) this.pump("datagram");
        }
        if (this.messages.length) {
            const {lane, bytes} = this.messages.shift();
            this.reading.delete(lane);
            return bytes;
        }
        if (this.waiter) throw new Error("another room read is pending");
        return new Promise((resolve, reject) => { this.waiter = { resolve, reject }; });
    }

    async writeHint(bytes) {
        if (this.stopped || bytes.length > 1024) throw closed();
        if (!this.hintsOut || this.wt.datagrams.maxDatagramSize < bytes.length)
            return this.write(bytes);
        await this.hintsOut.write(bytes);
    }

    async writeStream(bytes) {
        if (this.stopped || bytes.length > MAX_FRAME) throw closed();
        if (!this.wt || !this.single) return this.write(bytes);
        const writable = await this.wt.createUnidirectionalStream();
        const writer = writable.getWriter();
        const frame = new Uint8Array(8 + bytes.length);
        frame.set([72, 89, 85, 49]);
        new DataView(frame.buffer).setUint32(4, bytes.length);
        frame.set(bytes, 8);
        try {
            await writer.write(frame);
            await writer.close();
        } finally { writer.releaseLock(); }
    }

    async write(bytes) {
        if (this.stopped || bytes.length > MAX_FRAME) throw closed();
        if (this.wt) {
            const frame = new Uint8Array(4 + bytes.length);
            new DataView(frame.buffer).setUint32(0, bytes.length);
            frame.set(bytes, 4);
            await this.writer.write(frame);
        } else {
            if (this.ws.bufferedAmount >= MAX_FRAME * 128) { this.close(); throw closed(); }
            this.ws.send(bytes);
        }
    }

    close() {
        if (this.stopped) return;
        this.stopped = true;
        clearTimeout(this.timer);
        this.rejectOpen?.(closed());
        this.waiter?.reject(closed());
        this.waiter = undefined;
        this.messages.length = 0;
        this.hintsIn?.cancel().catch(() => {});
        this.hintsOut?.abort().catch(() => {});
        this.reader?.cancel().catch(() => {});
        this.writer?.abort().catch(() => {});
        this.wt?.close();
        this.ws?.close();
    }
}
