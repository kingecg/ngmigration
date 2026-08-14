import { Component } from '@angular/core';
import { Store } from '@ngrx/store';

@Component({
  selector: 'app-root',
  templateUrl: './app.component.html',
  styleUrls: ['./app.component.css'],
})
export class AppComponent {
  title = 'ng12-app';
  loaded = true;
  items = ['alpha', 'beta', 'gamma'];

  constructor(private store: Store) {}

  ngOnInit(): void {
    this.store.dispatch({ type: '[App] Init' });
  }
}
