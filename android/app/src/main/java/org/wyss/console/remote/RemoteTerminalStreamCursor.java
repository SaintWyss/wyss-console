package org.wyss.console.remote;

/**
 * Volatile terminal stream cursor for the future authenticated gateway adapter.
 * It tracks offsets only: output bytes, prompts and credentials are never retained here.
 */
public final class RemoteTerminalStreamCursor {
    public static final int MAX_FRAME_BYTES = 16 * 1024;

    public enum State {
        DISCONNECTED,
        AWAITING_CHECKPOINT,
        READY,
        GAP
    }

    public enum Result {
        ATTACHMENT_RESET,
        CHECKPOINT_ACCEPTED,
        FRAME_ACCEPTED,
        STALE_FRAME_IGNORED,
        GAP_DETECTED,
        STALE_SESSION,
        INVALID
    }

    private State state = State.DISCONNECTED;
    private long generation;
    private long epoch;
    private long nextOffset;
    private long gapFloorOffset;

    public synchronized State state() {
        return state;
    }

    /** Starts a new volatile attachment. Reconnect always discards the prior cursor. */
    public synchronized Result reconnect(long nextGeneration, long nextEpoch) {
        State previousState = state;
        long previousGeneration = generation;
        long previousEpoch = epoch;
        disconnect();
        if (nextGeneration <= 0 || nextEpoch <= 0) return Result.INVALID;
        if (previousState != State.DISCONNECTED
                && nextGeneration == previousGeneration
                && nextEpoch <= previousEpoch) return Result.STALE_SESSION;
        generation = nextGeneration;
        epoch = nextEpoch;
        nextOffset = 0;
        gapFloorOffset = 0;
        state = State.AWAITING_CHECKPOINT;
        return Result.ATTACHMENT_RESET;
    }

    public synchronized void disconnect() {
        state = State.DISCONNECTED;
        generation = 0;
        epoch = 0;
        nextOffset = 0;
        gapFloorOffset = 0;
    }

    /** A checkpoint is an offset anchor, not proof that this client has a rendered screen. */
    public synchronized Result checkpoint(
            long frameGeneration, long connectionEpoch, long frameOffset, String screenDigest) {
        if (state == State.DISCONNECTED || frameGeneration != generation || connectionEpoch != epoch) {
            return Result.STALE_SESSION;
        }
        if (frameOffset < 0 || !validDigest(screenDigest)) return Result.INVALID;
        if (state == State.READY && frameOffset != nextOffset) {
            state = State.GAP;
            gapFloorOffset = Math.max(nextOffset, frameOffset);
            return Result.GAP_DETECTED;
        }
        if (state == State.GAP && frameOffset < gapFloorOffset) return Result.INVALID;
        nextOffset = frameOffset;
        gapFloorOffset = frameOffset;
        state = State.READY;
        return Result.CHECKPOINT_ACCEPTED;
    }

    /** Accepts only contiguous frames. A gap requires a fresh checkpoint. */
    public synchronized Result frame(
            long frameGeneration, long connectionEpoch, long offset, int byteLength) {
        if (state == State.DISCONNECTED || frameGeneration != generation || connectionEpoch != epoch) {
            return Result.STALE_SESSION;
        }
        if (byteLength <= 0 || byteLength > MAX_FRAME_BYTES || offset < 0) {
            if (state == State.READY || state == State.GAP) {
                state = State.GAP;
                if (offset >= 0) gapFloorOffset = Math.max(gapFloorOffset, offset);
            }
            return Result.INVALID;
        }
        if (Long.MAX_VALUE - offset < byteLength) {
            state = State.GAP;
            gapFloorOffset = Long.MAX_VALUE;
            return Result.INVALID;
        }
        long frameEnd = offset + byteLength;
        if (state == State.GAP) {
            gapFloorOffset = Math.max(gapFloorOffset, frameEnd);
            return Result.GAP_DETECTED;
        }
        if (state != State.READY) return Result.INVALID;
        if (offset < nextOffset) {
            if (frameEnd > nextOffset) {
                state = State.GAP;
                gapFloorOffset = Math.max(gapFloorOffset, frameEnd);
                return Result.GAP_DETECTED;
            }
            return Result.STALE_FRAME_IGNORED;
        }
        if (offset > nextOffset) {
            state = State.GAP;
            gapFloorOffset = Math.max(gapFloorOffset, frameEnd);
            return Result.GAP_DETECTED;
        }
        nextOffset += byteLength;
        gapFloorOffset = nextOffset;
        return Result.FRAME_ACCEPTED;
    }

    private static boolean validDigest(String digest) {
        if (digest == null || digest.length() != 64) return false;
        for (int i = 0; i < digest.length(); i++) {
            char c = digest.charAt(i);
            if (!((c >= '0' && c <= '9') || (c >= 'a' && c <= 'f') || (c >= 'A' && c <= 'F'))) {
                return false;
            }
        }
        return true;
    }
}
