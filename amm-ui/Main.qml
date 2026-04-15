import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15

Item {
  width: 400
  height: 300

  ColumnLayout {
    anchors.centerIn: parent
    spacing: 12

    Text {
      Layout.alignment: Qt.AlignHCenter
      text: "Hello from ui_qml_example!"
      font.pixelSize: 18
    }

    Button {
      Layout.alignment: Qt.AlignHCenter
      text: "Call Core Module"
      onClicked: {
        // The logos bridge is injected by the host application.
        // Uncomment to call a backend module:
        // var result = logos.callModule("my_module", "myMethod", ["arg"])
        console.log("Button clicked")
      }
    }
  }
}
