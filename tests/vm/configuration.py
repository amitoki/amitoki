"""VMごとの中継設定を生成する。"""

import json


def relay_options(node, relay, prefix):
    from settings import NODES, P2P_PORTS, P2P_GUEST_PORT
    if relay != "p2p":
        return ""
    configuration = (f'[{prefix}.options]\nlisten = "0.0.0.0:{P2P_GUEST_PORT}"\n'
                     f'certificate = "/opt/amitoki-lab/identity/{node}.der"\n'
                     'private_key = "/opt/amitoki-lab/identity/key.der"\n')
    for peer in NODES:
        if peer != node:
            configuration += (f'[[{prefix}.options.peers]]\nnode_id = "vm-{peer}"\n'
                              f'address = "10.0.2.2:{P2P_PORTS[peer]}"\n'
                              f'certificate = "/opt/amitoki-lab/identity/{peer}.der"\n')
    return configuration


def relay_configuration(node, relay, pipeline=False):
    configuration = (f'node_id = "vm-{node}"\nchannel = "vm-lab"\ninterface = "relay0"\npromiscuous = true\n'
                     '[firewall]\npolicy = "blacklist"\nrules = []\n')
    if not pipeline:
        return configuration + f'[relay]\nplugin = "{relay}"\n' + relay_options(node, relay, "relay")
    relays = ["postgres", "p2p"] if relay == "both" else [relay]
    for plugin in relays:
        configuration += f'[[pipeline.relays]]\nid = "{plugin}"\nplugin = "{plugin}"\n'
        configuration += relay_options(node, plugin, "pipeline.relays")
    for instance in ("outbound", "inbound", "audit"):
        configuration += (f'[[pipeline.blocks]]\nid = "{instance}"\nplugin = "packet-rules"\n'
                          f'on_error = "{"drop_branch" if instance == "audit" else "stop"}"\n'
                          f'[pipeline.blocks.options]\nlabel = "{instance}"\nlog_every = 128\n')
        if instance != "audit":
            # 試験用EtherType 0x88b5は拒否する。ARPとIPv4/IPv6を通す。
            configuration += 'allowed_ether_types = [2048, 2054, 34525, 34998]\n'
    routes = [("capture", ["outbound", "audit"]), ("outbound.pass", relays), ("outbound.drop", []),
              ("inbound.pass", ["inject"]), ("inbound.drop", []), ("audit.pass", []), ("audit.drop", [])]
    routes += [(f"{plugin}.received", ["inbound"]) for plugin in relays]
    for source, targets in routes:
        configuration += f'[[pipeline.routes]]\nfrom = "{source}"\nto = {json.dumps(targets)}\n'
    return configuration
