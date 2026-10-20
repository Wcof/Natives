# Isolated HTTPS CONNECT fixture

These DER certificates and the unencrypted key are disposable local test identities, not product or user credentials. The leaf certificate covers localhost/127.0.0.1 and is trusted only by the explicit test client. Production clients keep Mozilla roots and public-address guards; no system trust store is changed. The fixture expires in October 2036.
