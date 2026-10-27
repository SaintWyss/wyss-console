package org.wyss.console.remote;

/** Transport-neutral view of the host gateway. No production transport is selected or connected. */
public interface RemoteGatewayClient {
    enum State { DISCONNECTED, CONNECTING, CONNECTED, RECONNECTING, BLOCKED, EXPIRED, REVOKED }

    State state();

    /** The shipped M7 client is explicitly offline until OD01 and its reviewed transport ADR. */
    final class Unavailable implements RemoteGatewayClient {
        @Override public State state() { return State.BLOCKED; }
    }
}
