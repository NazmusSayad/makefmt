set dotenv-command := 'echo KEY=command'
set dotenv-override := true

@foo:
  echo $KEY
