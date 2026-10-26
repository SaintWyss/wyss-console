package org.wyss.console.remote;

import java.util.Arrays;
import java.util.UUID;

/** Volatile, no-replay terminal input contract. Nothing from this class is persisted. */
public final class RemoteInputSequencer {
    public static final int MAX_INPUT_BYTES = 4096;

    public enum Delivery {
        ACCEPTED,
        PARTIAL,
        REJECTED,
        OUTCOME_UNKNOWN,
        NEW_INPUT_REQUIRED
    }

    public static final class InputRequest {
        public final String requestId;
        public final long sessionGeneration;
        public final long connectionEpoch;
        public final long sequence;
        private byte[] bytes;
        private final int byteCount;
        private boolean sent;
        private boolean closed;

        private InputRequest(long generation, long epoch, long sequence, byte[] bytes) {
            this.requestId = UUID.randomUUID().toString();
            this.sessionGeneration = generation;
            this.connectionEpoch = epoch;
            this.sequence = sequence;
            this.bytes = Arrays.copyOf(bytes, bytes.length);
            this.byteCount = bytes.length;
        }

        public synchronized int requestedBytes() {
            return byteCount;
        }

        public synchronized byte[] payloadForFirstSend() {
            if (closed || sent || bytes == null) {
                throw new IllegalStateException("input_already_sent");
            }
            sent = true;
            byte[] payload = Arrays.copyOf(bytes, byteCount);
            erase();
            return payload;
        }

        /** Closes the handle and snapshots delivery state in the same critical section. */
        private synchronized boolean invalidateAndGetSent() {
            boolean wasSent = sent;
            closed = true;
            erase();
            return wasSent;
        }

        private void erase() {
            if (bytes != null) {
                Arrays.fill(bytes, (byte) 0);
                bytes = null;
            }
        }
    }

    public static final class Result {
        public final Delivery delivery;
        /** Null when the remote outcome or accepted byte count is unknown. */
        public final Integer acceptedBytes;
        /** Null when the request cannot be correlated to a known in-flight payload. */
        public final Integer requestedBytes;
        public final String requestId;

        private Result(Delivery delivery, Integer acceptedBytes, Integer requestedBytes, String requestId) {
            this.delivery = delivery;
            this.acceptedBytes = acceptedBytes;
            this.requestedBytes = requestedBytes;
            this.requestId = requestId;
        }
    }

    private InputRequest inFlight;
    private long nextSequence = 1;

    public synchronized InputRequest begin(byte[] bytes, long generation, long epoch) {
        if (bytes == null || bytes.length == 0 || bytes.length > MAX_INPUT_BYTES) {
            throw new IllegalArgumentException("input_size_invalid");
        }
        if (generation <= 0 || epoch <= 0 || inFlight != null) {
            throw new IllegalStateException("input_not_admitted");
        }
        inFlight = new InputRequest(generation, epoch, nextSequence++, bytes);
        return inFlight;
    }

    /** An acknowledgement for another request is ambiguous; it cannot free or resend input. */
    public synchronized Result acknowledge(String requestId, int acceptedBytes) {
        if (inFlight == null || !inFlight.requestId.equals(requestId)) {
            return new Result(Delivery.OUTCOME_UNKNOWN, null, null, requestId);
        }
        InputRequest completed = inFlight;
        inFlight = null;
        int requestedBytes = completed.requestedBytes();
        boolean wasSent = completed.invalidateAndGetSent();
        if (!wasSent || acceptedBytes < 0 || acceptedBytes > requestedBytes) {
            return new Result(Delivery.OUTCOME_UNKNOWN, null, requestedBytes, requestId);
        }
        Delivery delivery = acceptedBytes == requestedBytes
                ? Delivery.ACCEPTED
                : acceptedBytes == 0 ? Delivery.REJECTED : Delivery.PARTIAL;
        return new Result(delivery, acceptedBytes, requestedBytes, requestId);
    }

    /** A lost ACK permanently consumes the volatile request; reconnect must use fresh user input. */
    public synchronized Result loseAcknowledgement() {
        if (inFlight == null) {
            return new Result(Delivery.NEW_INPUT_REQUIRED, null, null, "");
        }
        InputRequest uncertain = inFlight;
        inFlight = null;
        boolean wasSent = uncertain.invalidateAndGetSent();
        int requestedBytes = uncertain.requestedBytes();
        return new Result(
                wasSent ? Delivery.OUTCOME_UNKNOWN : Delivery.NEW_INPUT_REQUIRED,
                null,
                requestedBytes,
                uncertain.requestId);
    }

    /** A changed generation/epoch never reattaches queued bytes from the previous connection. */
    public synchronized Result reconnect(long generation, long epoch) {
        if (inFlight == null) {
            return new Result(Delivery.NEW_INPUT_REQUIRED, null, null, "");
        }
        InputRequest stale = inFlight;
        inFlight = null;
        // A generation or epoch change does not resolve whether the old host consumed bytes.
        // Once sent, every reconnect is ambiguous; before the first send, no external effect exists.
        boolean wasSent = stale.invalidateAndGetSent();
        int requestedBytes = stale.requestedBytes();
        Delivery delivery = wasSent
                ? Delivery.OUTCOME_UNKNOWN
                : Delivery.NEW_INPUT_REQUIRED;
        return new Result(delivery, null, requestedBytes, stale.requestId);
    }

    /** Cancellation releases the request handle; a sent request remains externally ambiguous. */
    public synchronized Result cancel(String requestId) {
        if (inFlight == null || !inFlight.requestId.equals(requestId)) {
            return new Result(Delivery.OUTCOME_UNKNOWN, null, null, requestId);
        }
        InputRequest cancelled = inFlight;
        inFlight = null;
        boolean wasSent = cancelled.invalidateAndGetSent();
        int requestedBytes = cancelled.requestedBytes();
        return new Result(
                wasSent ? Delivery.OUTCOME_UNKNOWN : Delivery.NEW_INPUT_REQUIRED,
                null,
                requestedBytes,
                cancelled.requestId);
    }
}
