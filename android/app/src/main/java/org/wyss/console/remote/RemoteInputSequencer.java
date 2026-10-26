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
        private final byte[] bytes;
        private boolean sent;

        private InputRequest(long generation, long epoch, long sequence, byte[] bytes) {
            this.requestId = UUID.randomUUID().toString();
            this.sessionGeneration = generation;
            this.connectionEpoch = epoch;
            this.sequence = sequence;
            this.bytes = Arrays.copyOf(bytes, bytes.length);
        }

        public int requestedBytes() {
            return bytes.length;
        }

        public synchronized boolean wasSent() {
            return sent;
        }

        public synchronized byte[] payloadForFirstSend() {
            if (sent) {
                throw new IllegalStateException("input_already_sent");
            }
            sent = true;
            return Arrays.copyOf(bytes, bytes.length);
        }
    }

    public static final class Result {
        public final Delivery delivery;
        public final int acceptedBytes;
        public final int requestedBytes;
        public final String requestId;

        private Result(Delivery delivery, int acceptedBytes, int requestedBytes, String requestId) {
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
            return new Result(Delivery.OUTCOME_UNKNOWN, 0, 0, requestId);
        }
        InputRequest completed = inFlight;
        inFlight = null;
        if (acceptedBytes < 0 || acceptedBytes > completed.requestedBytes()) {
            return new Result(Delivery.OUTCOME_UNKNOWN, 0, completed.requestedBytes(), requestId);
        }
        Delivery delivery = acceptedBytes == completed.requestedBytes()
                ? Delivery.ACCEPTED
                : acceptedBytes == 0 ? Delivery.REJECTED : Delivery.PARTIAL;
        return new Result(delivery, acceptedBytes, completed.requestedBytes(), requestId);
    }

    /** A lost ACK permanently consumes the volatile request; reconnect must use fresh user input. */
    public synchronized Result loseAcknowledgement() {
        if (inFlight == null) {
            return new Result(Delivery.NEW_INPUT_REQUIRED, 0, 0, "");
        }
        InputRequest uncertain = inFlight;
        inFlight = null;
        return new Result(Delivery.OUTCOME_UNKNOWN, 0, uncertain.requestedBytes(), uncertain.requestId);
    }

    /** A changed generation/epoch never reattaches queued bytes from the previous connection. */
    public synchronized Result reconnect(long generation, long epoch) {
        if (inFlight == null) {
            return new Result(Delivery.NEW_INPUT_REQUIRED, 0, 0, "");
        }
        InputRequest stale = inFlight;
        inFlight = null;
        // A generation or epoch change does not resolve whether the old host consumed bytes.
        // Once sent, every reconnect is ambiguous; before the first send, no external effect exists.
        Delivery delivery = stale.wasSent()
                ? Delivery.OUTCOME_UNKNOWN
                : Delivery.NEW_INPUT_REQUIRED;
        return new Result(delivery, 0, stale.requestedBytes(), stale.requestId);
    }
}
