// This client lives inside the container. Stdout is only the framed protocol.
import net from 'node:net';
let pending = Buffer.alloc(0); let socket; let connected = false;
process.stdin.on('data',chunk => {
  if (connected) socket.write(chunk);
  else { pending = Buffer.concat([pending,chunk]); if (pending.length > 160 * 1024) process.exit(2); }
});
process.stdin.on('end',() => socket?.end());
const deadline = Date.now()+10000;
function connect() {
  socket = net.createConnection('/tmp/roadeep-computer.sock');
  socket.on('connect',() => { connected = true; socket.write(pending); pending = Buffer.alloc(0); });
  socket.on('data',chunk => process.stdout.write(chunk));
  socket.on('end',() => process.exit(0));
  socket.on('error',() => { if (connected || Date.now() > deadline) process.exit(2); else setTimeout(connect,150); });
}
connect();
