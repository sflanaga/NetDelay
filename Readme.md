### NetDelay

Measure delay between a client and server over tcp.

Basic time echos are sent from client and upon or immediate
response from server the delay is recorded.

A ticker (off by default) can periodically write 
statistics on the recents delays.

Running server - bind to the IP you want allowed to outside world.
```
NetDelay.exe -s <your servers IP>
```

Running client - with 10 milli-second between echos and stats
written every 5 seconds
```
.\NetDelay.exe -c 127.0.0.1 -t 10s -i 10ms
```

You can use `-l debug` to get the individual echo timings.

The client will attempt to reconnect to server if that connection is lost.

The server spawns a thread per client to server more than one client.

