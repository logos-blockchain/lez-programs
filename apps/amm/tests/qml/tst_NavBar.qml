pragma ComponentBehavior: Bound

import QtQuick
import QtTest

import "../../qml" as Amm

Item {
    id: root

    width: 320
    height: 56

    Component {
        id: navComponent

        Amm.NavBar {
            width: root.width
            height: root.height
        }
    }

    TestCase {
        name: "NavBar"
        when: windowShown

        function test_compactControlsStayInsideViewport() {
            const nav = createTemporaryObject(navComponent, root)
            verify(nav)
            const trade = findChild(nav, "navTab0")
            const liquidity = findChild(nav, "navTab1")
            const wallet = findChild(nav, "walletConnectButton")
            verify(trade)
            verify(liquidity)
            verify(wallet)

            const controls = [trade, liquidity, wallet]
            for (let index = 0; index < controls.length; ++index) {
                const position = controls[index].mapToItem(nav, 0, 0)
                verify(position.x >= 0)
                verify(position.x + controls[index].width <= nav.width)
            }
            verify(trade.mapToItem(nav, trade.width, 0).x
                   <= liquidity.mapToItem(nav, 0, 0).x)
            verify(liquidity.mapToItem(nav, liquidity.width, 0).x
                   <= wallet.mapToItem(nav, 0, 0).x)
        }
    }
}
