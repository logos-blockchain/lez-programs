pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts

import "../shared"

ColumnLayout {
    id: root

    required property var theme
    property bool hasToken: false
    property string tokenColor: root.theme.colors.noTokenCircle
    property string tokenLetter: ""
    property string tokenText: qsTr("Select token")
    property string balance: ""
    property string accessibleName: qsTr("Select token")
    property bool invalid: false

    signal clicked

    spacing: 2

    Text {
        Layout.fillWidth: true
        visible: root.balance.length > 0
        text: qsTr("Balance %1").arg(root.balance)
        color: root.theme.colors.textSecondary
        font.pixelSize: 10
        horizontalAlignment: Text.AlignRight
        elide: Text.ElideRight
    }

    AmmTokenSelectButton {
        objectName: "tokenSelectButton"
        Layout.fillWidth: true
        theme: root.theme
        enabled: root.enabled
        invalid: root.invalid
        hasToken: root.hasToken
        tokenColor: root.tokenColor
        tokenLetter: root.tokenLetter
        text: root.tokenText
        maximumTextWidth: 112
        Accessible.name: root.accessibleName
        onClicked: root.clicked()
    }
}
