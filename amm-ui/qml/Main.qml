import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15
import "pages"

Item {
    id: root

    NavBar {
        id: navbar
        anchors.top:   parent.top
        anchors.left:  parent.left
        anchors.right: parent.right
        z: 100
    }

    Item {
        anchors.top:    navbar.bottom
        anchors.left:   parent.left
        anchors.right:  parent.right
        anchors.bottom: parent.bottom

        SwapPage {
            anchors.fill: parent
            visible: navbar.currentIndex === 0
        }

        LiquidityPage {
            anchors.fill: parent
            visible: navbar.currentIndex === 1
        }
    }
}
