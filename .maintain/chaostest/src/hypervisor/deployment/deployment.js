const k8s = require('./k8s')
const CONFIG = require('../../config')
const { pollUntil } = require('../utils/wait')
const { getBootNodeUrl } = require('../utils/utils')

const readOrCreateNamespace = async (namespace) => {
    try {
        console.log('Reading namespace')
        await k8s.readNameSpace(namespace)  // if namespace is available, do not create here
    } catch (error) {
        console.log('Namespace not present, creating...')
        await k8s.createNameSpace(namespace)
    }
    CONFIG.setNameSpace(namespace)
}
const createAlice = async (image, port) => {
    const substrateArgs = [
        '--chain=local',
        "--node-key",
        "0000000000000000000000000000000000000000000000000000000000000001",
        '--validator',
        "--no-telemetry",
        "--rpc-cors",
        "all",
        '--alice']
    const nodeSpec = {
        nodeId: 'alice',
        image,
        port,
        args: substrateArgs
    }
    nodeSpec.extraInfo = {
        nodeType: 'bootnode',
        privateKey: '',
        publicKey: '',
        peerId: '12D3KooWEyoppNCUx8Yx66oV9fJnriXwCcXwDDUA2kj6vnc6iDEp'
    }
    await createNode(nodeSpec)
}

const createBob = async (image, port) => {
    const substrateArgs = [
        '--chain=local',
        "--node-key",
        "0000000000000000000000000000000000000000000000000000000000000002",
        '--validator',
        '--bob',
        "--no-telemetry",
        "--rpc-cors",
        "all",
        '--bootnodes',
        getBootNodeUrl(CONFIG.bootnode)]
    let nodeSpec = {
        nodeId: 'bob',
        image,
        port,
        args: substrateArgs
    }
    nodeSpec.extraInfo = {
        nodeType: 'validator',
        privateKey: '',
        publicKey: ''
    }
    await createNode(nodeSpec)
}

const createAliceBobNodes = async (image, port) => {
    await createAlice(image, port)
    await createBob(image, port)
}

const createDevNode = async (image, port) => {
    const substrateArgs = ['--dev', '--rpc-external', '--ws-external']
    const nodeSpec = {
        nodeId: 'node-1',
        image,
        port,
        args: substrateArgs
    }
    await createNode(nodeSpec)
}

const createNode = async (nodeSpec) => {
    console.log(`Creating ${nodeSpec.nodeId} as ${nodeSpec.extraInfo? nodeSpec.extraInfo.nodeType: 'FullNode'} in ${CONFIG.namespace}`)
    await k8s.createPod(nodeSpec, CONFIG.namespace)
    console.log('Polling pod status')
    const pod = await pollUntil(
        () => k8s.getPod(nodeSpec.nodeId, CONFIG.namespace)
    )
    let nodeInfo = {
        podName: nodeSpec.nodeId,
        ip: pod.status.podIP,
        port: nodeSpec.port
    }
    if (nodeSpec.extraInfo) {
        Object.assign(nodeInfo, nodeSpec.extraInfo)
    }
    CONFIG.addNode(nodeInfo)
}

const cleanup = async (namespace) => {
    await k8s.deleteNameSpace(namespace)
    if (namespace === CONFIG.namespace) {
        CONFIG.reset()
    }
}

module.exports = {readOrCreateNamespace, createDevNode, createAliceBobNodes, cleanup}
