# Gemini APIでツールを呼び出す方法

Gemini APIでは、function calling（ツール呼び出し）機能を利用して、AIが外部ツールや関数を呼び出すことができます。  
主な手順は以下の通りです。

## 1. ツールの定義

リクエスト時に、呼び出したいツール（関数）の仕様をJSONで定義します。  
例:
```json
{
  "tools": [
    {
      "function_declarations": [
        {
          "name": "getWeather",
          "description": "指定した都市の天気を取得します",
          "parameters": {
            "type": "object",
            "properties": {
              "location": {
                "type": "string",
                "description": "都市名"
              }
            },
            "required": ["location"]
          }
        }
      ]
    }
  ]
}
```

## 2. リクエスト送信

APIリクエストのbodyに、ツール定義とプロンプト（AIへの指示文）を含めて送信します。

## 3. レスポンスの処理

Gemini APIは、ツール呼び出しが必要な場合、functionCallオブジェクトを含むレスポンスを返します。  
この内容を元に、外部ツールや関数を実行し、結果をAIに返します。

## コーディングエージェントとしての活用例

例えば、以下のようなfunction callingリクエストを送信します。

### サンプル: Pythonコード生成ツール

```json
  {
    "contents": [
      {
        "role": "user",
        "parts": [
          {
            "text": "Schedule a meeting with Bob and Alice for 03/27/2025 at 10:00 AM about the Q3 planning."
          }
        ]
      }
    ],
    "tools": [
      {
        "functionDeclarations": [
          {
            "name": "schedule_meeting",
            "description": "Schedules a meeting with specified attendees at a given time and date.",
            "parameters": {
              "type": "object",
              "properties": {
                "attendees": {
                  "type": "array",
                  "items": {"type": "string"},
                  "description": "List of people attending the meeting."
                },
                "date": {
                  "type": "string",
                  "description": "Date of the meeting (e.g., '2024-07-29')"
                },
                "time": {
                  "type": "string",
                  "description": "Time of the meeting (e.g., '15:00')"
                },
                "topic": {
                  "type": "string",
                  "description": "The subject or topic of the meeting."
                }
              },
              "required": ["attendees", "date", "time", "topic"]
            }
          }
        ]
      }
    ]
  }
```

Gemini APIは、functionCallレスポンスで「generatePythonCode」関数の呼び出し内容を返します。  

## 参考

詳細は公式ドキュメント:  
https://ai.google.dev/gemini-api/docs/function-calling?hl=ja
