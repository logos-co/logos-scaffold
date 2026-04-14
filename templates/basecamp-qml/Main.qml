import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Rectangle {
    id: root
    color: "#f5f2e8"

    property int counter: 0
    property string bridgeStatus: "Bridge not queried yet"

    function queryBridge() {
        if (typeof logos === "undefined" || !logos.callModule) {
            bridgeStatus = "Logos bridge unavailable"
            return
        }

        try {
            bridgeStatus = String(logos.callModule("package_manager", "getValidVariants", []))
        } catch (error) {
            bridgeStatus = String(error)
        }
    }

    ScrollView {
        anchors.fill: parent
        contentWidth: availableWidth

        ColumnLayout {
            width: parent.width
            spacing: 18
            anchors.margins: 24

            Rectangle {
                Layout.fillWidth: true
                radius: 20
                color: "#15332d"
                implicitHeight: 180

                ColumnLayout {
                    anchors.fill: parent
                    anchors.margins: 24
                    spacing: 12

                    Text {
                        text: "{{project_title}}"
                        color: "#f7f3e8"
                        font.pixelSize: 28
                        font.weight: Font.DemiBold
                    }

                    Text {
                        text: "A pure QML Basecamp plugin scaffolded by logos-scaffold."
                        color: "#d8d3c7"
                        wrapMode: Text.WordWrap
                        Layout.fillWidth: true
                    }

                    RowLayout {
                        spacing: 12

                        Button {
                            text: "Increment"
                            onClicked: root.counter += 1
                        }

                        Label {
                            text: "Counter: " + root.counter
                            color: "#f7f3e8"
                            font.pixelSize: 16
                        }
                    }
                }
            }

            Rectangle {
                Layout.fillWidth: true
                radius: 16
                color: "#ffffff"
                border.color: "#d7cfbf"
                implicitHeight: 220

                ColumnLayout {
                    anchors.fill: parent
                    anchors.margins: 20
                    spacing: 12

                    Text {
                        text: "Local Loop"
                        color: "#22322d"
                        font.pixelSize: 20
                        font.weight: Font.DemiBold
                    }

                    Text {
                        text: "Use `logos-scaffold build` to stage the plugin bundle, then `logos-scaffold install` to sync it into your local Basecamp data directory."
                        color: "#44544e"
                        wrapMode: Text.WordWrap
                        Layout.fillWidth: true
                    }

                    Button {
                        text: "Query package_manager bridge"
                        onClicked: root.queryBridge()
                    }

                    TextArea {
                        Layout.fillWidth: true
                        Layout.fillHeight: true
                        readOnly: true
                        wrapMode: TextEdit.Wrap
                        text: root.bridgeStatus
                    }
                }
            }
        }
    }
}
