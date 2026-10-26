package org.wyss.console.remote;

import android.hardware.biometrics.BiometricPrompt;
import android.security.keystore.KeyGenParameterSpec;
import android.security.keystore.KeyInfo;
import android.security.keystore.KeyProperties;

import java.math.BigInteger;
import java.security.KeyFactory;
import java.security.KeyPair;
import java.security.KeyPairGenerator;
import java.security.KeyStore;
import java.security.Signature;
import java.security.spec.ECGenParameterSpec;
import java.util.Arrays;


/** Device P-256 keys are non-exportable Android Keystore entries; this does not authenticate a host. */
public final class AndroidRemoteIdentity {
    private static final String STORE = "AndroidKeyStore";
    private static final String PREFIX = "wyss_remote_v2_";

    public static final class Keys {
        public final String aliasId;
        public final byte[] connectionPublicKey;
        public final byte[] presencePublicKey;
        public final int connectionSecurityLevel;
        public final int presenceSecurityLevel;
        /** False until a challenge-bound Android key attestation is verified by the host. */
        public final boolean attested;

        private Keys(String aliasId, byte[] connection, byte[] presence, int connectionLevel, int presenceLevel) {
            this.aliasId = aliasId;
            this.connectionPublicKey = connection;
            this.presencePublicKey = presence;
            this.connectionSecurityLevel = connectionLevel;
            this.presenceSecurityLevel = presenceLevel;
            this.attested = false;
        }
    }

    /** One prompt-bound, single-use presence operation. The caller must pass its CryptoObject to
     * BiometricPrompt.authenticate and complete only with that prompt's successful result. */
    public static final class PreparedPresence {
        private Signature signature;
        private byte[] challenge;
        private boolean handedToPrompt;
        private boolean consumed;

        private PreparedPresence(Signature signature, byte[] challenge) {
            this.signature = signature;
            this.challenge = Arrays.copyOf(challenge, challenge.length);
        }

        public synchronized BiometricPrompt.CryptoObject cryptoObject() {
            if (consumed || handedToPrompt || signature == null) {
                throw new IllegalStateException("presence_operation_unavailable");
            }
            handedToPrompt = true;
            return new BiometricPrompt.CryptoObject(signature);
        }

        public synchronized byte[] complete(BiometricPrompt.CryptoObject authenticated) throws Exception {
            if (consumed || !handedToPrompt || signature == null || authenticated == null
                    || authenticated.getSignature() != signature) {
                invalidate();
                throw new IllegalStateException("presence_authentication_not_bound");
            }
            try {
                signature.update(challenge);
                return signature.sign();
            } finally {
                invalidate();
            }
        }

        /** A cancelled or abandoned prompt cannot later be used to sign. */
        public synchronized void cancel() {
            invalidate();
        }

        private void invalidate() {
            consumed = true;
            signature = null;
            if (challenge != null) {
                Arrays.fill(challenge, (byte) 0);
                challenge = null;
            }
        }
    }

    private AndroidRemoteIdentity() {}

    public static Keys getOrCreate(String hostOpaqueId) throws Exception {
        if (hostOpaqueId == null || !hostOpaqueId.matches("[A-Za-z0-9_-]{1,64}")) {
            throw new IllegalArgumentException("host_id_invalid");
        }
        String aliasId = PREFIX + hostOpaqueId;
        KeyStore store = KeyStore.getInstance(STORE);
        store.load(null);
        String connectionAlias = aliasId + "_connection";
        String presenceAlias = aliasId + "_presence";
        ensureKey(store, connectionAlias, false);
        ensureKey(store, presenceAlias, true);
        KeyPair connection = keyPair(store, connectionAlias);
        KeyPair presence = keyPair(store, presenceAlias);
        return new Keys(
                aliasId,
                sec1(connection.getPublic()),
                sec1(presence.getPublic()),
                securityLevel(connection.getPrivate()),
                securityLevel(presence.getPrivate()));
    }

    /** Prepare the exact signature operation that must be authenticated by BiometricPrompt. */
    public static PreparedPresence preparePresence(String aliasId, byte[] challenge) throws Exception {
        if (aliasId == null || !aliasId.matches(PREFIX + "[A-Za-z0-9_-]{1,64}")) {
            throw new IllegalArgumentException("host_id_invalid");
        }
        if (challenge == null || challenge.length == 0 || challenge.length > 8192) {
            throw new IllegalArgumentException("challenge_size_invalid");
        }
        KeyStore store = KeyStore.getInstance(STORE);
        store.load(null);
        java.security.Key key = store.getKey(aliasId + "_presence", null);
        if (key == null) throw new IllegalStateException("presence_key_missing");
        Signature signature = Signature.getInstance("SHA256withECDSA");
        signature.initSign((java.security.PrivateKey) key);
        return new PreparedPresence(signature, challenge);
    }

    private static void ensureKey(KeyStore store, String alias, boolean presence) throws Exception {
        if (store.containsAlias(alias)) return;
        KeyPairGenerator generator = KeyPairGenerator.getInstance("EC", STORE);
        KeyGenParameterSpec.Builder spec = new KeyGenParameterSpec.Builder(
                alias, KeyProperties.PURPOSE_SIGN | KeyProperties.PURPOSE_VERIFY)
                .setAlgorithmParameterSpec(new ECGenParameterSpec("secp256r1"))
                .setDigests(KeyProperties.DIGEST_SHA256)
                .setUserAuthenticationRequired(presence);
        if (presence) {
            spec.setUserAuthenticationParameters(
                    0, KeyProperties.AUTH_BIOMETRIC_STRONG);
            spec.setInvalidatedByBiometricEnrollment(true);
        }
        generator.initialize(spec.build());
        generator.generateKeyPair();
    }

    private static KeyPair keyPair(KeyStore store, String alias) throws Exception {
        java.security.cert.Certificate certificate = store.getCertificate(alias);
        java.security.Key privateKey = store.getKey(alias, null);
        if (certificate == null || privateKey == null) throw new IllegalStateException("keystore_key_missing");
        return new KeyPair(certificate.getPublicKey(), (java.security.PrivateKey) privateKey);
    }

    private static byte[] sec1(java.security.PublicKey key) {
        if (!(key instanceof java.security.interfaces.ECPublicKey ec)) {
            throw new IllegalArgumentException("keystore_curve_unexpected");
        }
        byte[] point = new byte[65];
        point[0] = 0x04;
        fixed32(ec.getW().getAffineX(), point, 1);
        fixed32(ec.getW().getAffineY(), point, 33);
        return point;
    }

    private static void fixed32(BigInteger coordinate, byte[] output, int offset) {
        byte[] raw = coordinate.toByteArray();
        int start = raw.length == 33 && raw[0] == 0 ? 1 : 0;
        int count = raw.length - start;
        if (count > 32) throw new IllegalArgumentException("keystore_point_invalid");
        Arrays.fill(output, offset, offset + 32, (byte) 0);
        System.arraycopy(raw, start, output, offset + 32 - count, count);
    }

    private static int securityLevel(java.security.PrivateKey key) throws Exception {
        KeyFactory factory = KeyFactory.getInstance(key.getAlgorithm(), STORE);
        KeyInfo info = factory.getKeySpec(key, KeyInfo.class);
        return info.getSecurityLevel();
    }
}
