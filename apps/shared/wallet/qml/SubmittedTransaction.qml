import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

Item {
    id: root

    property string title: qsTr("Transaction submitted")
    property string transactionId: ""
    readonly property bool base58Shape: root.transactionId.length > 0
        && /^[1-9A-HJ-NP-Za-km-z]+$/.test(root.transactionId)

    implicitWidth: 420
    implicitHeight: resultCard.implicitHeight

    TextEdit {
        id: clipboardProxy
        visible: false
    }

    function copyTransactionId() {
        if (!root.transactionId)
            return
        clipboardProxy.text = root.transactionId
        clipboardProxy.selectAll()
        clipboardProxy.copy()
        clipboardProxy.deselect()
        clipboardProxy.text = ""
    }

    Rectangle {
        id: resultCard
        anchors.left: parent.left
        anchors.right: parent.right
        implicitHeight: resultContent.implicitHeight + 32
        color: "#18181b"
        border.color: "#3f3f46"
        border.width: 1
        radius: 8

        ColumnLayout {
            id: resultContent
            anchors.fill: parent
            anchors.margins: 16
            spacing: 12

            Label {
                Layout.fillWidth: true
                text: root.title
                color: "#f4f4f5"
                font.bold: true
                font.pixelSize: 17
                wrapMode: Text.WordWrap
            }

            Label {
                Layout.fillWidth: true
                text: qsTr("Transaction ID")
                color: "#a1a1aa"
                font.pixelSize: 12
            }

            RowLayout {
                Layout.fillWidth: true
                spacing: 8

                Label {
                    id: transactionLabel
                    objectName: "submittedTransactionId"
                    Layout.fillWidth: true
                    text: root.transactionId
                    color: "#f4f4f5"
                    font.family: "monospace"
                    wrapMode: Text.WrapAnywhere
                    Accessible.name: qsTr("Transaction ID %1").arg(root.transactionId)
                }

                CopyButton {
                    objectName: "copySubmittedTransactionButton"
                    enabled: root.transactionId.length > 0
                    onCopyRequested: root.copyTransactionId()
                }
            }
        }
    }
}
