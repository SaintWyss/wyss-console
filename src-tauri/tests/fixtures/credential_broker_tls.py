"""Disposable process E2E. This harness is not included in a product package.
The product broker's public main origin allowlist is unchanged; only this test's internal
Broker constructor trusts the local synthetic TLS endpoint/certificate.
"""
import json, pathlib, sys, tempfile, ssl, socket, threading, subprocess, http.client
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[2] / 'claude-bridge'))
from broker import Broker
cfg = json.loads(sys.stdin.buffer.readline(4097))
assert cfg['origin'] == 'https://api.anthropic.com'
secret = cfg.pop('api_key')
received = []
class Provider(BaseHTTPRequestHandler):
    def log_message(self, *args): pass
    def do_POST(self):
        self.rfile.read(int(self.headers['content-length']))
        received.append(dict(self.headers))
        body=b'{"ok":true}'
        self.send_response(200); self.send_header('content-length',str(len(body))); self.end_headers(); self.wfile.write(body)
class UnixHTTPConnection(http.client.HTTPConnection):
    def connect(self):
        self.sock=socket.socket(socket.AF_UNIX);self.sock.settimeout(5);self.sock.connect(self.host)
with tempfile.TemporaryDirectory(prefix='wyss-broker-tls-') as certdir:
    crt,pem=pathlib.Path(certdir)/'cert.pem',pathlib.Path(certdir)/'private.pem'
    subprocess.run(['openssl','req','-x509','-newkey','rsa:2048','-nodes','-keyout',str(pem),'-out',str(crt),'-days','1','-subj','/CN=127.0.0.1','-addext','subjectAltName=IP:127.0.0.1'],check=True,capture_output=True)
    server=ThreadingHTTPServer(('127.0.0.1',0),Provider)
    tls=ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER);tls.load_cert_chain(crt,pem);server.socket=tls.wrap_socket(server.socket,server_side=True)
    thread=threading.Thread(target=server.serve_forever,daemon=True);thread.start()
    origin='https://127.0.0.1:'+str(server.server_address[1])
    def request(broker,token):
        c=UnixHTTPConnection(broker.path);c.request('POST','/v1/messages',json.dumps({'model':'synthetic','tools':[]}),{'x-wyss-broker':token,'x-api-key':'untrusted-client-key','authorization':'untrusted-client-authorization'});r=c.getresponse();body=r.read();status=r.status;c.close();assert secret.encode() not in body;return status
    try:
        broker=Broker(directory=cfg['dir'],token=cfg['token'],origin=origin,profile=cfg['profile'],api_key=secret,_origins=(origin,)).start()
        try: assert request(broker,cfg['token'])==502;assert not received
        finally: broker.stop()
        broker=Broker(directory=cfg['dir'],token=cfg['token'],origin=origin,profile=cfg['profile'],api_key=secret,_origins=(origin,),_context=ssl.create_default_context(cafile=crt)).start()
        try:
            assert request(broker,'ab'*32)==401;assert not received
            assert request(broker,cfg['token'])==200
            assert len(received)==1
            headers={k.lower():v for k,v in received[0].items()}
            assert headers['x-api-key']==secret and 'authorization' not in headers
        finally: broker.stop()
        assert not pathlib.Path(broker.path).exists()
    finally: server.shutdown();server.server_close();thread.join(timeout=5)
print('broker_tls_delivery_passed')
