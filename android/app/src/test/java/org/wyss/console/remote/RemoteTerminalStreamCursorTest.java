package org.wyss.console.remote;

import org.junit.Test;

import static org.junit.Assert.assertEquals;

public final class RemoteTerminalStreamCursorTest {
    private static final String DIGEST = "a".repeat(64);

    @Test public void framesRequireCheckpointAndAdvanceOnlyContiguously() {
        RemoteTerminalStreamCursor cursor = new RemoteTerminalStreamCursor();
        assertEquals(RemoteTerminalStreamCursor.Result.ATTACHMENT_RESET, cursor.reconnect(7, 2));
        assertEquals(RemoteTerminalStreamCursor.Result.INVALID, cursor.frame(7, 2, 0, 3));
        assertEquals(RemoteTerminalStreamCursor.Result.CHECKPOINT_ACCEPTED,
                cursor.checkpoint(7, 2, 11, DIGEST));
        assertEquals(RemoteTerminalStreamCursor.Result.FRAME_ACCEPTED, cursor.frame(7, 2, 11, 3));
        assertEquals(RemoteTerminalStreamCursor.Result.FRAME_ACCEPTED, cursor.frame(7, 2, 14, 2));
        assertEquals(RemoteTerminalStreamCursor.State.READY, cursor.state());
    }

    @Test public void oldFramesAreIgnoredAndGapRequiresFreshCheckpoint() {
        RemoteTerminalStreamCursor cursor = attachedAt(5);
        assertEquals(RemoteTerminalStreamCursor.Result.FRAME_ACCEPTED, cursor.frame(7, 2, 5, 3));
        assertEquals(RemoteTerminalStreamCursor.Result.STALE_FRAME_IGNORED, cursor.frame(7, 2, 5, 3));
        assertEquals(RemoteTerminalStreamCursor.Result.GAP_DETECTED, cursor.frame(7, 2, 12, 2));
        assertEquals(RemoteTerminalStreamCursor.State.GAP, cursor.state());
        assertEquals(RemoteTerminalStreamCursor.Result.GAP_DETECTED, cursor.frame(7, 2, 8, 2));
        assertEquals(RemoteTerminalStreamCursor.Result.CHECKPOINT_ACCEPTED,
                cursor.checkpoint(7, 2, 20, DIGEST));
        assertEquals(RemoteTerminalStreamCursor.Result.FRAME_ACCEPTED, cursor.frame(7, 2, 20, 1));
    }

    @Test public void partiallyOverlappingFrameIsAGapRatherThanAnAcceptedDuplicate() {
        RemoteTerminalStreamCursor cursor = attachedAt(5);
        assertEquals(RemoteTerminalStreamCursor.Result.FRAME_ACCEPTED, cursor.frame(7, 2, 5, 3));
        assertEquals(RemoteTerminalStreamCursor.Result.GAP_DETECTED, cursor.frame(7, 2, 7, 3));
        assertEquals(RemoteTerminalStreamCursor.State.GAP, cursor.state());
    }

    @Test public void checkpointsCannotRewindAnActiveOrGappedCursor() {
        RemoteTerminalStreamCursor cursor = attachedAt(5);
        assertEquals(RemoteTerminalStreamCursor.Result.FRAME_ACCEPTED, cursor.frame(7, 2, 5, 3));
        assertEquals(RemoteTerminalStreamCursor.Result.GAP_DETECTED,
                cursor.checkpoint(7, 2, 5, DIGEST));
        assertEquals(RemoteTerminalStreamCursor.Result.INVALID, cursor.checkpoint(7, 2, 7, DIGEST));
        assertEquals(RemoteTerminalStreamCursor.Result.CHECKPOINT_ACCEPTED,
                cursor.checkpoint(7, 2, 8, DIGEST));
        assertEquals(RemoteTerminalStreamCursor.Result.FRAME_ACCEPTED, cursor.frame(7, 2, 8, 1));
    }

    @Test public void reconnectRejectsStaleEpochAndNeverCarriesPriorOffset() {
        RemoteTerminalStreamCursor cursor = attachedAt(30);
        assertEquals(RemoteTerminalStreamCursor.Result.STALE_SESSION, cursor.reconnect(7, 2));
        assertEquals(RemoteTerminalStreamCursor.State.DISCONNECTED, cursor.state());
        assertEquals(RemoteTerminalStreamCursor.Result.STALE_SESSION, cursor.reconnect(7, 2));
        assertEquals(RemoteTerminalStreamCursor.Result.STALE_SESSION, cursor.frame(7, 2, 30, 1));
        assertEquals(RemoteTerminalStreamCursor.Result.ATTACHMENT_RESET, cursor.reconnect(7, 3));
        assertEquals(RemoteTerminalStreamCursor.State.AWAITING_CHECKPOINT, cursor.state());
        assertEquals(RemoteTerminalStreamCursor.Result.STALE_SESSION, cursor.frame(7, 2, 30, 1));
        assertEquals(RemoteTerminalStreamCursor.Result.CHECKPOINT_ACCEPTED,
                cursor.checkpoint(7, 3, 0, DIGEST));
        assertEquals(RemoteTerminalStreamCursor.Result.FRAME_ACCEPTED, cursor.frame(7, 3, 0, 1));
    }

    @Test public void explicitDisconnectDoesNotForgetTheAcceptedEpochWatermark() {
        RemoteTerminalStreamCursor cursor = attachedAt(4);
        cursor.disconnect();
        assertEquals(RemoteTerminalStreamCursor.State.DISCONNECTED, cursor.state());
        assertEquals(RemoteTerminalStreamCursor.Result.STALE_SESSION, cursor.reconnect(7, 2));
        assertEquals(RemoteTerminalStreamCursor.Result.STALE_SESSION, cursor.reconnect(7, 2));
        assertEquals(RemoteTerminalStreamCursor.Result.ATTACHMENT_RESET, cursor.reconnect(7, 3));
    }

    @Test public void generationChangeRequiresFreshCheckpoint() {
        RemoteTerminalStreamCursor cursor = attachedAt(9);
        assertEquals(RemoteTerminalStreamCursor.Result.ATTACHMENT_RESET, cursor.reconnect(8, 2));
        assertEquals(RemoteTerminalStreamCursor.Result.CHECKPOINT_ACCEPTED,
                cursor.checkpoint(8, 2, 0, DIGEST));
        assertEquals(RemoteTerminalStreamCursor.Result.STALE_SESSION, cursor.frame(7, 2, 0, 1));
    }

    @Test public void invalidCheckpointAndFrameBoundariesFailClosed() {
        RemoteTerminalStreamCursor cursor = new RemoteTerminalStreamCursor();
        assertEquals(RemoteTerminalStreamCursor.Result.INVALID, cursor.reconnect(0, 1));
        assertEquals(RemoteTerminalStreamCursor.Result.ATTACHMENT_RESET, cursor.reconnect(7, 2));
        assertEquals(RemoteTerminalStreamCursor.Result.INVALID, cursor.checkpoint(7, 2, -1, DIGEST));
        assertEquals(RemoteTerminalStreamCursor.Result.INVALID, cursor.checkpoint(7, 2, 0, "bad"));
        assertEquals(RemoteTerminalStreamCursor.Result.CHECKPOINT_ACCEPTED,
                cursor.checkpoint(7, 2, Long.MAX_VALUE - 1, DIGEST));
        assertEquals(RemoteTerminalStreamCursor.Result.INVALID, cursor.frame(7, 2, Long.MAX_VALUE - 1, 2));
        assertEquals(RemoteTerminalStreamCursor.State.GAP, cursor.state());
        assertEquals(RemoteTerminalStreamCursor.Result.INVALID, cursor.checkpoint(7, 2, 0, DIGEST));

        RemoteTerminalStreamCursor bounded = attachedAt(0);
        assertEquals(RemoteTerminalStreamCursor.Result.INVALID, bounded.frame(7, 2, 0, 0));
        assertEquals(RemoteTerminalStreamCursor.State.GAP, bounded.state());
        assertEquals(RemoteTerminalStreamCursor.Result.CHECKPOINT_ACCEPTED,
                bounded.checkpoint(7, 2, 0, DIGEST));
        assertEquals(RemoteTerminalStreamCursor.Result.INVALID,
                bounded.frame(7, 2, 0, RemoteTerminalStreamCursor.MAX_FRAME_BYTES + 1));
        assertEquals(RemoteTerminalStreamCursor.State.GAP, bounded.state());
    }

    private static RemoteTerminalStreamCursor attachedAt(long offset) {
        RemoteTerminalStreamCursor cursor = new RemoteTerminalStreamCursor();
        assertEquals(RemoteTerminalStreamCursor.Result.ATTACHMENT_RESET, cursor.reconnect(7, 2));
        assertEquals(RemoteTerminalStreamCursor.Result.CHECKPOINT_ACCEPTED,
                cursor.checkpoint(7, 2, offset, DIGEST));
        return cursor;
    }
}
