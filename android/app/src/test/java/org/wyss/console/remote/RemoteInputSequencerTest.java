package org.wyss.console.remote;

import org.junit.Test;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertThrows;

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
        assertEquals(2, result.acceptedBytes);
        assertEquals(4, result.requestedBytes);
        assertEquals(2, sequencer.begin(new byte[] {3, 4}, 7, 4).sequence);
    }

    @Test public void lostAcknowledgementAndReconnectNeverReplayOldBytes() {
        RemoteInputSequencer sequencer = new RemoteInputSequencer();
        RemoteInputSequencer.InputRequest request = sequencer.begin(new byte[] {8, 9}, 12, 3);
        request.payloadForFirstSend();
        RemoteInputSequencer.Result result = sequencer.loseAcknowledgement();
        assertEquals(RemoteInputSequencer.Delivery.OUTCOME_UNKNOWN, result.delivery);
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
        sequencer.begin(new byte[] {3}, 12, 3);
        assertEquals(RemoteInputSequencer.Delivery.NEW_INPUT_REQUIRED, sequencer.reconnect(13, 4).delivery);
    }

    @Test public void aStaleAcknowledgementCannotCompleteCurrentInput() {
        RemoteInputSequencer sequencer = new RemoteInputSequencer();
        RemoteInputSequencer.InputRequest request = sequencer.begin(new byte[] {5}, 2, 9);
        request.payloadForFirstSend();
        assertEquals(RemoteInputSequencer.Delivery.OUTCOME_UNKNOWN,
                sequencer.acknowledge("older-request", 1).delivery);
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
        assertEquals(0, result.acceptedBytes);
        assertThrows(IllegalStateException.class, request::payloadForFirstSend);
    }
}
