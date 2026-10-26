package org.wyss.console.remote;

import org.junit.Test;

import static org.junit.Assert.assertArrayEquals;
import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertNull;
import static org.junit.Assert.assertThrows;

import java.util.concurrent.CountDownLatch;
import java.util.concurrent.atomic.AtomicReference;

public final class RemoteInputSequencerTest {
    @Test public void payloadCanBeReadForOneSendOnly() {
        RemoteInputSequencer sequencer = new RemoteInputSequencer();
        RemoteInputSequencer.InputRequest request = sequencer.begin(new byte[] {1, 2, 3}, 7, 4);
        assertEquals(3, request.payloadForFirstSend().length);
        assertThrows(IllegalStateException.class, request::payloadForFirstSend);
    }

    @Test public void partialAcknowledgementReportsExactCountAndDoesNotSendRemainder() {
        RemoteInputSequencer sequencer = new RemoteInputSequencer();
        RemoteInputSequencer.InputRequest request = sequencer.begin(new byte[] {1, 2, 3, 4}, 7, 4);
        request.payloadForFirstSend();

        RemoteInputSequencer.Result result = sequencer.acknowledge(request.requestId, 2);
        assertEquals(RemoteInputSequencer.Delivery.PARTIAL, result.delivery);
        assertEquals(Integer.valueOf(2), result.acceptedBytes);
        assertEquals(Integer.valueOf(4), result.requestedBytes);
        assertEquals(2, sequencer.begin(new byte[] {3, 4}, 7, 4).sequence);
    }

    @Test public void lostAcknowledgementAndReconnectNeverReplayOldBytes() {
        RemoteInputSequencer sequencer = new RemoteInputSequencer();
        RemoteInputSequencer.InputRequest request = sequencer.begin(new byte[] {8, 9}, 12, 3);
        request.payloadForFirstSend();
        RemoteInputSequencer.Result result = sequencer.loseAcknowledgement();
        assertEquals(RemoteInputSequencer.Delivery.OUTCOME_UNKNOWN, result.delivery);
        assertNull(result.acceptedBytes);
        assertEquals(Integer.valueOf(2), result.requestedBytes);
        assertEquals(RemoteInputSequencer.Delivery.NEW_INPUT_REQUIRED, sequencer.reconnect(12, 4).delivery);
        assertThrows(IllegalStateException.class, request::payloadForFirstSend);
        assertEquals(2, sequencer.begin(new byte[] {8, 9}, 12, 4).sequence);
    }

    @Test public void sentInputRemainsUnknownAcrossAChangedGenerationOrEpoch() {
        RemoteInputSequencer sequencer = new RemoteInputSequencer();
        RemoteInputSequencer.InputRequest request = sequencer.begin(new byte[] {3}, 12, 3);
        request.payloadForFirstSend();
        assertEquals(RemoteInputSequencer.Delivery.OUTCOME_UNKNOWN, sequencer.reconnect(13, 4).delivery);
        assertEquals(2, sequencer.begin(new byte[] {3}, 13, 4).sequence);
    }

    @Test public void unsentInputCanBeDroppedWithoutReportingAnExternalEffect() {
        RemoteInputSequencer sequencer = new RemoteInputSequencer();
        RemoteInputSequencer.InputRequest request = sequencer.begin(new byte[] {3}, 12, 3);
        assertEquals(RemoteInputSequencer.Delivery.NEW_INPUT_REQUIRED, sequencer.reconnect(13, 4).delivery);
        assertThrows(IllegalStateException.class, request::payloadForFirstSend);
    }

    @Test public void staleUnsentHandleCannotReleasePayloadAfterReconnectWithFreshInput() {
        RemoteInputSequencer sequencer = new RemoteInputSequencer();
        RemoteInputSequencer.InputRequest stale = sequencer.begin(new byte[] {1, 2}, 12, 3);

        assertEquals(RemoteInputSequencer.Delivery.NEW_INPUT_REQUIRED,
                sequencer.reconnect(12, 4).delivery);
        RemoteInputSequencer.InputRequest fresh = sequencer.begin(new byte[] {7, 8}, 12, 4);

        assertThrows(IllegalStateException.class, stale::payloadForFirstSend);
        assertArrayEquals(new byte[] {7, 8}, fresh.payloadForFirstSend());
        assertEquals(RemoteInputSequencer.Delivery.ACCEPTED,
                sequencer.acknowledge(fresh.requestId, 2).delivery);
    }

    @Test public void aStaleAcknowledgementCannotCompleteCurrentInput() {
        RemoteInputSequencer sequencer = new RemoteInputSequencer();
        RemoteInputSequencer.InputRequest request = sequencer.begin(new byte[] {5}, 2, 9);
        request.payloadForFirstSend();
        RemoteInputSequencer.Result stale = sequencer.acknowledge("older-request", 1);
        assertEquals(RemoteInputSequencer.Delivery.OUTCOME_UNKNOWN, stale.delivery);
        assertNull(stale.acceptedBytes);
        assertNull(stale.requestedBytes);
        assertEquals(RemoteInputSequencer.Delivery.ACCEPTED,
                sequencer.acknowledge(request.requestId, 1).delivery);
    }

    @Test public void invalidInputLengthsFailBeforeRequestCreation() {
        RemoteInputSequencer sequencer = new RemoteInputSequencer();
        assertThrows(IllegalArgumentException.class, () -> sequencer.begin(new byte[0], 1, 1));
        assertThrows(IllegalArgumentException.class,
                () -> sequencer.begin(new byte[RemoteInputSequencer.MAX_INPUT_BYTES + 1], 1, 1));
        RemoteInputSequencer.InputRequest boundary = sequencer.begin(
                new byte[RemoteInputSequencer.MAX_INPUT_BYTES], 1, 1);
        assertEquals(RemoteInputSequencer.MAX_INPUT_BYTES, boundary.requestedBytes());
    }

    @Test public void impossibleAcceptedCountIsUnknownAndCannotBeRetried() {
        RemoteInputSequencer sequencer = new RemoteInputSequencer();
        RemoteInputSequencer.InputRequest request = sequencer.begin(new byte[] {1, 2}, 1, 1);
        request.payloadForFirstSend();
        RemoteInputSequencer.Result result = sequencer.acknowledge(request.requestId, 3);
        assertEquals(RemoteInputSequencer.Delivery.OUTCOME_UNKNOWN, result.delivery);
        assertNull(result.acceptedBytes);
        assertEquals(Integer.valueOf(2), result.requestedBytes);
        assertThrows(IllegalStateException.class, request::payloadForFirstSend);
    }

    @Test public void cancellationInvalidatesTheHandleAndKeepsSentOutcomeUnknown() {
        RemoteInputSequencer sequencer = new RemoteInputSequencer();
        RemoteInputSequencer.InputRequest request = sequencer.begin(new byte[] {4, 5}, 1, 1);
        request.payloadForFirstSend();

        RemoteInputSequencer.Result result = sequencer.cancel(request.requestId);
        assertEquals(RemoteInputSequencer.Delivery.OUTCOME_UNKNOWN, result.delivery);
        assertNull(result.acceptedBytes);
        assertEquals(Integer.valueOf(2), result.requestedBytes);
        assertThrows(IllegalStateException.class, request::payloadForFirstSend);
    }

    @Test public void cancellationBeforeSendDropsBytesAndRequiresNewInput() {
        RemoteInputSequencer sequencer = new RemoteInputSequencer();
        RemoteInputSequencer.InputRequest request = sequencer.begin(new byte[] {7}, 1, 1);

        RemoteInputSequencer.Result result = sequencer.cancel(request.requestId);
        assertEquals(RemoteInputSequencer.Delivery.NEW_INPUT_REQUIRED, result.delivery);
        assertNull(result.acceptedBytes);
        assertEquals(Integer.valueOf(1), result.requestedBytes);
        assertThrows(IllegalStateException.class, request::payloadForFirstSend);
    }

    @Test public void acknowledgementBeforeFirstSendCannotClaimDelivery() {
        RemoteInputSequencer sequencer = new RemoteInputSequencer();
        RemoteInputSequencer.InputRequest request = sequencer.begin(new byte[] {6}, 1, 1);

        RemoteInputSequencer.Result result = sequencer.acknowledge(request.requestId, 1);
        assertEquals(RemoteInputSequencer.Delivery.OUTCOME_UNKNOWN, result.delivery);
        assertNull(result.acceptedBytes);
        assertEquals(Integer.valueOf(1), result.requestedBytes);
        assertThrows(IllegalStateException.class, request::payloadForFirstSend);
    }

    @Test public void reconnectAndFirstSendHaveOneAtomicOutcome() throws Exception {
        for (int attempt = 0; attempt < 250; attempt++) {
            RemoteInputSequencer sequencer = new RemoteInputSequencer();
            RemoteInputSequencer.InputRequest request = sequencer.begin(new byte[] {9}, 1, 1);
            CountDownLatch ready = new CountDownLatch(2);
            CountDownLatch start = new CountDownLatch(1);
            AtomicReference<Boolean> payloadReleased = new AtomicReference<>(false);

            Thread sender = new Thread(() -> {
                ready.countDown();
                try {
                    start.await();
                    request.payloadForFirstSend();
                    payloadReleased.set(true);
                } catch (IllegalStateException closedByReconnect) {
                    payloadReleased.set(false);
                } catch (InterruptedException interrupted) {
                    Thread.currentThread().interrupt();
                    throw new AssertionError(interrupted);
                }
            });
            AtomicReference<RemoteInputSequencer.Result> reconnectResult = new AtomicReference<>();
            Thread reconnect = new Thread(() -> {
                ready.countDown();
                try {
                    start.await();
                    reconnectResult.set(sequencer.reconnect(1, 2));
                } catch (InterruptedException interrupted) {
                    Thread.currentThread().interrupt();
                    throw new AssertionError(interrupted);
                }
            });

            sender.start();
            reconnect.start();
            ready.await();
            start.countDown();
            sender.join();
            reconnect.join();

            RemoteInputSequencer.Delivery expected = payloadReleased.get()
                    ? RemoteInputSequencer.Delivery.OUTCOME_UNKNOWN
                    : RemoteInputSequencer.Delivery.NEW_INPUT_REQUIRED;
            assertEquals(expected, reconnectResult.get().delivery);
            assertThrows(IllegalStateException.class, request::payloadForFirstSend);
        }
    }
}
