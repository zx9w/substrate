const fs = require('fs')
const path = require('path')
const configPath = path.join(__dirname, './config.json')

class Config {
    constructor() {
        this.load()
    }
    
    async load() {
        fs.readFile(configPath, (err, data) => {
            if (err) {
                if (err.code === 'ENOENT') {
                    this.reset()
                } else {
                    throw err
                }
            }  else {
                try {
                    Object.assign(this, JSON.parse(data));
                } catch (error) {
                    console.log(`config file is corrupted, resetting...`)
                    this.reset()
                }
            };
        });
    };

    getConfig () {
        return this
    }

    async update() {
        let data = JSON.stringify(this.getConfig());
        fs.writeFile(configPath, data, (err) => {
            if (err) throw err;
            console.log('Configuration updated');
        });
    }

    async setNameSpace(namespace) {
        this.namespace = namespace
        this.update()
    }

    async addNode(node) {
        if (!this.nodes || Array.isArray(this.nodes)) {
            this.nodes = []
        }
        if (node.nodeType === 'bootnode') {
            this.bootnode = node
        }
        this.nodes.push(node)
        this.update()
    }

    async reset() {
        let data = JSON.stringify({});
        fs.writeFile(configPath, data, (err) => {
            if (err) throw err;
            this.load()
        });
    }
}

const config = new Config()
module.exports = config
