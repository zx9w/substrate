const getBootNodeUrl = (bootnode) => {
    return `/dns4/${bootnode.ip}/tcp/30333/p2p/${bootnode.peerId}`
}

module.exports = {getBootNodeUrl}