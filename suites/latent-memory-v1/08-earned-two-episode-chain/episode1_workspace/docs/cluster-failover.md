# Leader Failover Fencing

Standby scheduler coordinators acquire monotonic epoch fencing tokens in etcd
before taking over partition dispatch loops.
