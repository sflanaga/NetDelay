## NetDelay

Measure delay between a client and server over tcp.

Basic time echos are sent from client and upon response from server that delay is recorded.

A ticker (off by default) can periodically write 
statistics on the recents delays.

### Usage:
Running server - bind to 0.0.0.0 as default server IP
```
NetDelay.exe -s 
```

Running client - with 10 milli-second between echoes and stats
are written every 10 seconds
```
.\NetDelay.exe -c <IP of your server> -t 10s -i 10ms
```

You can use `-l debug` to get the individual echo timings.

The client will attempt to reconnect to the server if that connection is lost.

The server spawns a thread per client to serve more than one client.

There are other options
