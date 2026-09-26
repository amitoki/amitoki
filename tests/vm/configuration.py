"""VMごとの中継設定を生成する。"""

def relay_configuration(node, relay):
    from settings import NODES, P2P_PORTS, P2P_GUEST_PORT
    configuration = (f'node_id = "vm-{node}"\nchannel = "vm-lab"\ninterface = "relay0"\npromiscuous = true\n'
                     f'[relay]\nplugin = "{relay}"\n[firewall]\npolicy = "blacklist"\nrules = []\n')
    if relay == "p2p":
        configuration += (f'[relay.options]\nlisten = "0.0.0.0:{P2P_GUEST_PORT}"\n'
                          f'certificate = "/opt/amitoki-lab/identity/{node}.der"\n'
                          'private_key = "/opt/amitoki-lab/identity/key.der"\n')
        for peer in NODES:
            if peer != node:
                configuration += (f'[[relay.options.peers]]\nnode_id = "vm-{peer}"\n'
                                  f'address = "10.0.2.2:{P2P_PORTS[peer]}"\n'
                                  f'certificate = "/opt/amitoki-lab/identity/{peer}.der"\n')
    return configuration
