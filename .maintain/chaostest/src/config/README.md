chaostest CONFIG
=========

Since deployment can behave differently, we want to keep a state between phases including different test subjects.

# Content
The state could include informations such as:
```
{
    namespace,
    image,
    bootnode: {
        podname,
        ip,
        port,
        peerId,
        privateKey,
        publicKey
    },
    nodes: [{
        podname,
        ip,
        port,
        validator,
        privateKey (validator only),
        publicKey (validator only)
    }]
}
```

# TODO
k8s configuration
chaos-agent
