const k8s = require('@kubernetes/client-node')
const CONFIG = require('../../config')

// load k8s
const kc = new k8s.KubeConfig()
kc.loadFromDefault()

// load k8s Apis
const k8sAppApi = kc.makeApiClient(k8s.AppsV1Api)
const k8sCoreApi = kc.makeApiClient(k8s.CoreV1Api)

const createNameSpace = async namespace => {
  const namespaceJson = {
    apiVersion: 'v1',
    kind: 'Namespace',
    metadata: {
      name: namespace,
    },
  }
  return await k8sCoreApi.createNamespace(namespaceJson)
}

const readNameSpace = async namespace => {
    return await k8sCoreApi.readNamespace(namespace)
}

const createPod = async (nodeSpec, namespace) => {
    const {label, nodeId, image, args, ports} = nodeSpec
    const spec = {
        metadata: {
          labels: {
            app: label,
          },
          name: nodeId
        },
        spec: {
          containers: [
            {
              image: image,
              imagePullPolicy: 'Always',
              name: nodeId,
              ports: ports,
              args: args
            }
          ]
        }
      }
    return await k8sCoreApi.createNamespacedPod(namespace, spec)
}

const getDeploymentStatus = async (deploymentName, namespace) => {
    const response = await k8sAppApi.readNamespacedDeploymentStatus(deploymentName, namespace)
    const status = response.response.body.status
    function getAvailability(item) {
        return item.type === 'Available';
    }
    if (status && status.conditions) {
        return status.conditions.find(getAvailability)
    }
    return undefined
}

const deleteNameSpace = async (namespace) => {
    console.log(`Taking down NameSpace ${namespace}...`)
    if (process.env.KEEP_NAMESPACE && process.env.KEEP_NAMESPACE === 1) {
        return
    }
    return k8sCoreApi.deleteNamespace(namespace)
}

const getNameSpacedPods = async (namespace) => {
    const response = await k8sCoreApi.listNamespacedPod(namespace)
    return response.body.items
}

const getPod = async (nodeId, namespace) => {
    const pods = await getNameSpacedPods(namespace)
    const found = pods.find(
        (pod) => !!pod.metadata && pod.metadata.name === nodeId && !!pod.status && pod.status.podIP
      );
    if (!found) {
        throw Error(`GetNode(${nodeId}): node is not present in the cluster`)
    }
    return found
}

const startForwardServer =  async (namespace, pod, port, onReady) => {
    const net = require('net');
    const forward = new k8s.PortForward(kc);

    // This simple server just forwards traffic from itself to a service running in kubernetes
    // -> localhost:8080 -> port-forward-tunnel -> kubernetes-pod
    // This is basically equivalent to 'kubectl port-forward ...' but in TypeScript.
    const server = net.createServer((socket) => {
        forward.portForward(namespace, pod, [port], socket, null, socket);
    });

    server.listen(port, '127.0.0.1', ()=> {
        console.log('Forwarding server started, ready to connect')
        onReady()
    });
}

module.exports = {createNameSpace, readNameSpace, createPod, deleteNameSpace, getDeploymentStatus, getPod, getNameSpacedPods, startForwardServer}